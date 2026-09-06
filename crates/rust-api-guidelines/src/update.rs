use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use tracing::{info, warn};

use crate::cache::GuidelineCache;
use crate::config::Config;
use crate::error::AppError;
use crate::model::{Category, Guideline};
use crate::parser;
use crate::search::SearchEngine;
use mcp_common::embedding::Embedder;
use mcp_common::vectordb::VectorDb;

pub struct UpdateResult {
    pub updated: bool,
    pub commit: String,
    pub guideline_count: usize,
    /// What happened at the remote before this update. Reported so a
    /// caller can tell "already current" from "never looked".
    pub remote_sync: String,
}

pub struct UpdateService {
    config: Config,
    embedder: Arc<Embedder>,
    vectordb: Arc<VectorDb>,
    cache: Arc<GuidelineCache>,
}

impl UpdateService {
    pub fn new(
        config: Config,
        embedder: Arc<Embedder>,
        vectordb: Arc<VectorDb>,
        cache: Arc<GuidelineCache>,
    ) -> Self {
        Self {
            config,
            embedder,
            vectordb,
            cache,
        }
    }

    pub fn get_repo_commit(&self) -> Result<String, AppError> {
        mcp_common::git::head_commit(&self.config.repo_path)
            .map_err(|e| AppError::Git(e.to_string()))
    }

    /// Fetch and fast-forward the corpus clone.
    ///
    /// Never fails the update: a remote that cannot be reached leaves the
    /// clone alone and returns an outcome the caller reports, so a stale
    /// index is visible rather than silent.
    fn sync_repo(&self) -> mcp_common::git::SyncOutcome {
        match mcp_common::git::sync(&self.config.repo_path, self.config.repo_auto_pull) {
            Ok(outcome) => outcome,
            Err(e) => mcp_common::git::SyncOutcome::Failed(e.to_string()),
        }
    }

    pub async fn needs_update(&self) -> Result<bool, AppError> {
        let current_commit = self.get_repo_commit()?;
        let cached_commit = self.cache.get_repo_commit().await;

        match cached_commit {
            Some(cached) if cached == current_commit => {
                let table_check = self
                    .vectordb
                    .get_by_id(SearchEngine::table_name(), "__nonexistent__")
                    .await;
                match table_check {
                    Ok(_) => Ok(false),
                    Err(_) => {
                        info!("LanceDB table missing, re-index needed");
                        Ok(true)
                    }
                }
            }
            _ => Ok(true),
        }
    }

    pub async fn full_reindex(
        &self,
    ) -> Result<(Vec<Guideline>, HashMap<String, Category>, String), AppError> {
        let current_commit = self.get_repo_commit()?;
        info!(commit = %current_commit, "starting full re-index");

        let (guidelines, categories) = parser::parse_guidelines_repo(&self.config.repo_path())?;
        info!(
            guideline_count = guidelines.len(),
            category_count = categories.len(),
            "parsed guidelines"
        );

        let embedding_texts: Vec<String> = guidelines
            .iter()
            .map(parser::compose_embedding_text)
            .collect();

        info!("generating embeddings for {} guidelines", guidelines.len());
        let embeddings = self.embedder.embed_documents(&embedding_texts).await?;

        if embeddings.len() != guidelines.len() {
            return Err(AppError::Common(mcp_common::error::CommonError::Embedding(
                format!(
                    "embedding count mismatch: expected {}, got {}",
                    guidelines.len(),
                    embeddings.len()
                ),
            )));
        }

        let batch = build_record_batch(&guidelines, &embedding_texts, &embeddings)?;
        let schema = batch.schema();

        self.vectordb
            .create_or_replace_table(SearchEngine::table_name(), schema, vec![batch])
            .await?;

        self.cache.invalidate_all().await;

        for g in &guidelines {
            self.cache.set_guideline(g).await;
        }

        let mut category_list: Vec<_> = categories.values().cloned().collect();
        category_list.sort_by(|a, b| a.key.cmp(&b.key));
        self.cache.set_categories(&category_list).await;

        for key in categories.keys() {
            let mut ids: Vec<String> = guidelines
                .iter()
                .filter(|g| &g.category == key)
                .map(|g| g.id.clone())
                .collect();
            ids.sort();
            self.cache.set_category_guideline_ids(key, &ids).await;
        }

        self.cache.set_repo_commit(&current_commit).await;

        info!(
            commit = %current_commit,
            guidelines = guidelines.len(),
            "re-index complete"
        );

        Ok((guidelines, categories, current_commit))
    }

    pub async fn update(
        &self,
    ) -> Result<(UpdateResult, Option<(Vec<Guideline>, HashMap<String, Category>)>), AppError> {
        // Contact the remote BEFORE reading HEAD. Reading first was the whole
        // defect: the commit check compared the clone against itself.
        let sync = self.sync_repo();
        match &sync {
            mcp_common::git::SyncOutcome::Failed(reason) => {
                warn!(%reason, "remote sync failed; serving local content")
            }
            outcome => info!(sync = %outcome, "repository sync"),
        }

        let current_commit = self.get_repo_commit()?;

        if !self.needs_update().await? {
            info!(commit = %current_commit, "guidelines up to date, skipping re-index");
            return Ok((
                UpdateResult {
                    updated: false,
                    commit: current_commit,
                    guideline_count: 0,
                    remote_sync: sync.to_string(),
                },
                None,
            ));
        }

        let (guidelines, categories, commit) = self.full_reindex().await?;
        let count = guidelines.len();

        Ok((
            UpdateResult {
                updated: true,
                commit,
                guideline_count: count,
                remote_sync: sync.to_string(),
            },
            Some((guidelines, categories)),
        ))
    }
}

fn build_record_batch(
    guidelines: &[Guideline],
    texts: &[String],
    embeddings: &[Vec<f32>],
) -> Result<RecordBatch, AppError> {
    let embedding_dim = 768i32;

    let ids: Vec<&str> = guidelines.iter().map(|g| g.id.as_str()).collect();
    let titles: Vec<&str> = guidelines.iter().map(|g| g.title.as_str()).collect();
    let categories: Vec<&str> = guidelines.iter().map(|g| g.category.as_str()).collect();
    let text_strs: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();

    let id_array: ArrayRef = Arc::new(StringArray::from(ids));
    let title_array: ArrayRef = Arc::new(StringArray::from(titles));
    let category_array: ArrayRef = Arc::new(StringArray::from(categories));
    let text_array: ArrayRef = Arc::new(StringArray::from(text_strs));

    let flat_values: Vec<f32> = embeddings.iter().flat_map(|e| e.iter().copied()).collect();
    let values_array = Float32Array::from(flat_values);
    let embedding_array: ArrayRef = Arc::new(
        FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            embedding_dim,
            Arc::new(values_array),
            None,
        )
        .map_err(|e| {
            AppError::Common(mcp_common::error::CommonError::VectorDb(format!(
                "failed to build embedding array: {e}"
            )))
        })?,
    );

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("category", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), embedding_dim),
            false,
        ),
    ]));

    RecordBatch::try_new(
        schema,
        vec![
            id_array,
            title_array,
            category_array,
            text_array,
            embedding_array,
        ],
    )
    .map_err(|e| {
        AppError::Common(mcp_common::error::CommonError::VectorDb(format!(
            "failed to build record batch: {e}"
        )))
    })
}
