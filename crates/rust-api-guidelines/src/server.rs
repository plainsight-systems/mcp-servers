use std::collections::HashMap;
use std::sync::Arc;

use rmcp::{
    Json, ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_handler, tool_router,
};
use tokio::sync::RwLock;
use tracing::info;

use crate::cache::GuidelineCache;
use crate::config::Config;
use crate::model::{Category, Guideline};
use crate::search::SearchEngine;
use crate::update::UpdateService;
use mcp_common::embedding::Embedder;
use mcp_common::mcp_api::{
    CategoryInfo, CategoryListResponse, GetGuidelineParams, GuidelineDetailResponse,
    GuidelineSearchResult, GuidelineSummary, ListCategoryParams, SearchGuidelinesParams,
    SearchGuidelinesResponse, UpdateGuidelinesResponse,
};
use mcp_common::vectordb::VectorDb;

pub struct AppState {
    pub guidelines: HashMap<String, Guideline>,
    pub categories: HashMap<String, Category>,
}

#[derive(Clone)]
pub struct RustApiGuidelinesServer {
    state: Arc<RwLock<AppState>>,
    search_engine: Arc<SearchEngine>,
    update_service: Arc<UpdateService>,
    cache: Arc<GuidelineCache>,
    tool_router: ToolRouter<RustApiGuidelinesServer>,
}

impl RustApiGuidelinesServer {
    pub fn new(
        guidelines: Vec<Guideline>,
        categories: HashMap<String, Category>,
        embedder: Arc<Embedder>,
        vectordb: Arc<VectorDb>,
        cache: Arc<GuidelineCache>,
        config: Config,
    ) -> Self {
        let guideline_map: HashMap<String, Guideline> = guidelines
            .into_iter()
            .map(|g| (g.id.clone(), g))
            .collect();

        let search_engine = Arc::new(SearchEngine::new(
            Arc::clone(&embedder),
            Arc::clone(&vectordb),
            Arc::clone(&cache),
        ));

        let update_service = Arc::new(UpdateService::new(
            config,
            Arc::clone(&embedder),
            Arc::clone(&vectordb),
            Arc::clone(&cache),
        ));

        let state = Arc::new(RwLock::new(AppState {
            guidelines: guideline_map,
            categories,
        }));

        Self {
            state,
            search_engine,
            update_service,
            cache,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl RustApiGuidelinesServer {
    #[tool(description = "Search Rust API guidelines by semantic similarity.")]
    async fn search_guidelines(
        &self,
        Parameters(params): Parameters<SearchGuidelinesParams>,
    ) -> Result<Json<SearchGuidelinesResponse>, String> {
        let query = params.query.trim().to_string();
        if query.is_empty() {
            return Err("query must not be empty".to_string());
        }

        let limit = params.limit.unwrap_or(10).min(50) as usize;

        let results = self
            .search_engine
            .search(&query, limit)
            .await
            .map_err(|e| format!("search failed: {e}"))?;

        let normalized: Vec<GuidelineSearchResult> = results
            .into_iter()
            .map(|r| GuidelineSearchResult {
                id: r.id,
                title: r.title,
                category: r.category,
                score: r.score,
                summary: r.summary,
            })
            .collect();

        Ok(Json(SearchGuidelinesResponse {
            results: normalized,
        }))
    }

    #[tool(description = "Get a Rust API guideline by ID (e.g. 'C-CASE', 'C-DEBUG').")]
    async fn get_guideline(
        &self,
        Parameters(params): Parameters<GetGuidelineParams>,
    ) -> Result<Json<GuidelineDetailResponse>, String> {
        let guideline_id = params.guideline_id.trim().to_string();
        if guideline_id.is_empty() {
            return Err("guideline_id must not be empty".to_string());
        }

        if let Some(cached) = self.cache.get_guideline(&guideline_id).await {
            return Ok(Json(to_api_guideline(&cached)));
        }

        let state = self.state.read().await;
        let guideline = state
            .guidelines
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(&guideline_id))
            .map(|(_, g)| g)
            .ok_or_else(|| format!("guideline not found: {guideline_id}"))?;

        Ok(Json(to_api_guideline(guideline)))
    }

    #[tool(description = "List all Rust API guidelines in a category (e.g. 'Naming', 'Documentation').")]
    async fn list_category(
        &self,
        Parameters(params): Parameters<ListCategoryParams>,
    ) -> Result<Json<CategoryListResponse>, String> {
        let category_key = params.category.trim().to_string();
        if category_key.is_empty() {
            return Err("category must not be empty".to_string());
        }

        let state = self.state.read().await;
        let (category_key, category) = state
            .categories
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(&category_key))
            .map(|(key, category)| (key.clone(), category.clone()))
            .ok_or_else(|| {
                let mut available: Vec<&str> = state.categories.keys().map(|s| s.as_str()).collect();
                available.sort_unstable();
                format!(
                    "unknown category: '{category_key}'. Available categories: {}",
                    available.join(", ")
                )
            })?;

        let mut guideline_summaries: Vec<GuidelineSummary> = state
            .guidelines
            .values()
            .filter(|g| g.category == category_key)
            .map(|g| GuidelineSummary {
                id: g.id.clone(),
                title: g.title.clone(),
            })
            .collect();
        guideline_summaries.sort_by(|a, b| a.id.cmp(&b.id));

        let response = CategoryListResponse {
            category: CategoryInfo {
                key: category.key.clone(),
                display_name: category.key,
                guideline_count: category.guideline_count,
            },
            guidelines: guideline_summaries,
        };

        Ok(Json(response))
    }

    #[tool(description = "Trigger a re-index of Rust API guidelines from the git repository.")]
    async fn update_guidelines(&self) -> Result<Json<UpdateGuidelinesResponse>, String> {
        info!("update_guidelines tool invoked");

        let (result, new_data) = self
            .update_service
            .update()
            .await
            .map_err(|e| format!("update failed: {e}"))?;

        if let Some((guidelines, categories)) = new_data {
            let guideline_count = guidelines.len();
            let guideline_map: HashMap<String, Guideline> = guidelines
                .into_iter()
                .map(|g| (g.id.clone(), g))
                .collect();

            let mut state = self.state.write().await;
            state.guidelines = guideline_map;
            state.categories = categories;
            info!(guideline_count, "in-memory state updated");
        }

        let response = UpdateGuidelinesResponse {
            updated: result.updated,
            commit: result.commit,
            remote_sync: result.remote_sync,
            guideline_count: if result.updated {
                result.guideline_count
            } else {
                let state = self.state.read().await;
                state.guidelines.len()
            },
        };

        Ok(Json(response))
    }
}

fn to_api_guideline(guideline: &Guideline) -> GuidelineDetailResponse {
    GuidelineDetailResponse {
        id: guideline.id.clone(),
        anchor: guideline.anchor.clone(),
        title: guideline.title.clone(),
        category: guideline.category.clone(),
        raw_markdown: guideline.raw_markdown.clone(),
        sections: None,
        source_file: Some(guideline.source_file.clone()),
    }
}

#[tool_handler]
impl ServerHandler for RustApiGuidelinesServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rust-api-guidelines".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Rust API Guidelines MCP server. Provides semantic search and lookup over the \
                 official Rust API Guidelines. Use search_guidelines for natural language queries, \
                 get_guideline for specific IDs (for example C-CASE), list_category for chapter \
                 browsing, and update_guidelines to refresh from the repository."
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RustApiGuidelinesServer;

    #[test]
    fn tools_publish_output_schemas() {
        let tools = RustApiGuidelinesServer::tool_router().list_all();
        for name in [
            "search_guidelines",
            "get_guideline",
            "list_category",
            "update_guidelines",
        ] {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("missing tool: {name}"));
            assert!(
                tool.output_schema.is_some(),
                "tool {name} should publish output_schema"
            );
        }
    }
}
