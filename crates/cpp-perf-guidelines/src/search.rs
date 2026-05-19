/// Search engine for the cpp-perf-guidelines corpus.
///
/// Embeds a query, performs vector search in LanceDB, and formats results.
/// Caches search results in Redis when available.
use std::sync::Arc;

use arrow_array::{Float32Array, RecordBatch, StringArray};
use tracing::{info, warn};

use crate::cache::GuidelineCache;
use crate::model::GuidelineResult;
use mcp_common::embedding::Embedder;
use mcp_common::vectordb::VectorDb;

/// LanceDB table name. Distinct from other guideline servers so multiple
/// servers can share one LanceDB directory without collision.
const VECTOR_TABLE_NAME: &str = "perf_guidelines";

pub struct SearchEngine {
    embedder: Arc<Embedder>,
    vectordb: Arc<VectorDb>,
    cache: Arc<GuidelineCache>,
}

impl SearchEngine {
    pub fn new(
        embedder: Arc<Embedder>,
        vectordb: Arc<VectorDb>,
        cache: Arc<GuidelineCache>,
    ) -> Self {
        Self {
            embedder,
            vectordb,
            cache,
        }
    }

    /// Search guidelines by semantic similarity to the query.
    ///
    /// Returns up to `limit` results, ranked by similarity. Results are cached
    /// in Redis for subsequent identical queries.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<GuidelineResult>, crate::error::AppError> {
        if let Some(cached) = self.cache.get_search_results(query, limit).await {
            info!(query, "search cache hit");
            return Ok(cached);
        }

        let query_embedding = self.embedder.embed_query(query).await?;

        let batches = self
            .vectordb
            .search(VECTOR_TABLE_NAME, &query_embedding, limit)
            .await?;

        let results = extract_search_results(&batches);

        self.cache.set_search_results(query, limit, &results).await;

        Ok(results)
    }

    /// Returns the LanceDB table name used for this corpus.
    pub fn table_name() -> &'static str {
        VECTOR_TABLE_NAME
    }
}

/// Extract `GuidelineResult` values from LanceDB search result batches.
///
/// Expected columns: id, title, category, summary (Utf8), `_distance` (Float32).
fn extract_search_results(batches: &[RecordBatch]) -> Vec<GuidelineResult> {
    let mut results = Vec::new();

    for batch in batches {
        let num_rows = batch.num_rows();
        let schema = batch.schema();

        let id_col = get_string_column(batch, &schema, "id");
        let title_col = get_string_column(batch, &schema, "title");
        let category_col = get_string_column(batch, &schema, "category");
        let summary_col = get_string_column(batch, &schema, "summary");
        let distance_col = get_float_column(batch, &schema, "_distance");

        let (Some(id_col), Some(title_col), Some(category_col), Some(summary_col)) =
            (id_col, title_col, category_col, summary_col)
        else {
            warn!("search result batch missing expected columns");
            continue;
        };

        for row in 0..num_rows {
            let distance: f32 = distance_col.map(|c| c.value(row)).unwrap_or(0.0);
            // LanceDB returns L2 distance (lower = more similar). Invert to a
            // similarity score clamped to [0, 1].
            let score: f32 = (1.0_f32 - distance).max(0.0);

            results.push(GuidelineResult {
                id: id_col.value(row).to_string(),
                title: title_col.value(row).to_string(),
                category: category_col.value(row).to_string(),
                score,
                summary: summary_col.value(row).to_string(),
            });
        }
    }

    results
}

fn get_string_column<'a>(
    batch: &'a RecordBatch,
    schema: &arrow_schema::Schema,
    name: &str,
) -> Option<&'a StringArray> {
    let idx = schema.index_of(name).ok()?;
    batch.column(idx).as_any().downcast_ref::<StringArray>()
}

fn get_float_column<'a>(
    batch: &'a RecordBatch,
    schema: &arrow_schema::Schema,
    name: &str,
) -> Option<&'a Float32Array> {
    let idx = schema.index_of(name).ok()?;
    batch.column(idx).as_any().downcast_ref::<Float32Array>()
}
