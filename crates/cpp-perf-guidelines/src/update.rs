/// Update service for the cpp-perf-guidelines corpus.
///
/// Checks the git repository state, re-parses and re-indexes when the commit
/// changes. Triggered at startup and on-demand via the `update_guidelines` tool.
use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use tracing::info;

use crate::cache::GuidelineCache;
use crate::config::Config;
use crate::error::AppError;
use crate::model::{Category, Guideline};
use crate::parser;
use crate::search::SearchEngine;
use mcp_common::embedding::Embedder;
use mcp_common::vectordb::VectorDb;

/// Embedding vector dimensionality produced by the embedding model.
const EMBEDDING_DIM: i32 = 768;

/// Result of an update operation.
pub struct UpdateResult {
    /// Whether an actual re-index occurred (false if already up-to-date).
    pub updated: bool,
    /// The current git commit hash.
    pub commit: String,
    /// Number of guidelines after the update.
    pub guideline_count: usize,
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

    /// Get the current git HEAD commit hash from the corpus repository.
    pub fn get_repo_commit(&self) -> Result<String, AppError> {
        let output = std::process::Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(&self.config.repo_path)
            .output()
            .map_err(|e| AppError::Git(format!("failed to run git rev-parse: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Git(format!("git rev-parse failed: {stderr}")));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Check if an update is needed by comparing the current commit with the
    /// cached one. Returns `true` if re-indexing should occur.
    pub async fn needs_update(&self) -> Result<bool, AppError> {
        let current_commit = self.get_repo_commit()?;
        let cached_commit = self.cache.get_repo_commit().await;

        match cached_commit {
            Some(cached) if cached == current_commit => {
                // Commit matches, but the LanceDB table may be absent.
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

    /// Perform a full re-index: parse, embed, store in LanceDB, populate caches.
    pub async fn full_reindex(
        &self,
    ) -> Result<(Vec<Guideline>, HashMap<String, Category>, String), AppError> {
        let current_commit = self.get_repo_commit()?;
        info!(commit = %current_commit, "starting full re-index");

        // 1. Parse the corpus.
        let (guidelines, categories) = parser::parse_corpus(self.config.repo_dir())?;
        info!(
            guideline_count = guidelines.len(),
            category_count = categories.len(),
            "parsed corpus"
        );

        // 2. Compose embedding texts.
        let embedding_texts: Vec<String> =
            guidelines.iter().map(parser::compose_embedding_text).collect();

        // 3. Generate embeddings (batched).
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

        // 4. Build the LanceDB record batch and (re)create the table.
        let batch = build_record_batch(&guidelines, &embedding_texts, &embeddings)?;
        let schema = batch.schema();
        self.vectordb
            .create_or_replace_table(SearchEngine::table_name(), schema, vec![batch])
            .await?;

        // 5. Invalidate caches and repopulate per-guideline entries.
        self.cache.invalidate_all().await;
        for g in &guidelines {
            self.cache.set_guideline(g).await;
        }
        self.cache.set_repo_commit(&current_commit).await;

        info!(
            commit = %current_commit,
            guidelines = guidelines.len(),
            "re-index complete"
        );

        Ok((guidelines, categories, current_commit))
    }

    /// Run a full update cycle: check if needed, then re-index if so.
    pub async fn update(
        &self,
    ) -> Result<(UpdateResult, Option<(Vec<Guideline>, HashMap<String, Category>)>), AppError> {
        let current_commit = self.get_repo_commit()?;

        if !self.needs_update().await? {
            info!(commit = %current_commit, "corpus up to date, skipping re-index");
            return Ok((
                UpdateResult {
                    updated: false,
                    commit: current_commit,
                    guideline_count: 0, // caller uses the existing count
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
            },
            Some((guidelines, categories)),
        ))
    }
}

/// Build an Arrow `RecordBatch` from parsed guidelines and their embeddings.
///
/// Columns: id, title, category, summary, text (embedding source), embedding.
fn build_record_batch(
    guidelines: &[Guideline],
    texts: &[String],
    embeddings: &[Vec<f32>],
) -> Result<RecordBatch, AppError> {
    let ids: Vec<&str> = guidelines.iter().map(|g| g.id.as_str()).collect();
    let titles: Vec<&str> = guidelines.iter().map(|g| g.title.as_str()).collect();
    let categories: Vec<&str> = guidelines.iter().map(|g| g.category.as_str()).collect();
    let summaries: Vec<&str> = guidelines.iter().map(|g| g.summary.as_str()).collect();
    let text_strs: Vec<&str> = texts.iter().map(|t| t.as_str()).collect();

    let id_array: ArrayRef = Arc::new(StringArray::from(ids));
    let title_array: ArrayRef = Arc::new(StringArray::from(titles));
    let category_array: ArrayRef = Arc::new(StringArray::from(categories));
    let summary_array: ArrayRef = Arc::new(StringArray::from(summaries));
    let text_array: ArrayRef = Arc::new(StringArray::from(text_strs));

    let flat_values: Vec<f32> = embeddings.iter().flat_map(|e| e.iter().copied()).collect();
    let values_array = Float32Array::from(flat_values);
    let embedding_array: ArrayRef = Arc::new(
        FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            EMBEDDING_DIM,
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
        Field::new("summary", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                EMBEDDING_DIM,
            ),
            false,
        ),
    ]));

    RecordBatch::try_new(
        schema,
        vec![
            id_array,
            title_array,
            category_array,
            summary_array,
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
