/// MCP server implementation for the cpp-perf-guidelines corpus.
///
/// Exposes four tools:
/// - `search_guidelines`: Semantic search over the corpus
/// - `get_guideline`: Look up a specific guideline by ID
/// - `list_category`: List all guidelines in a category
/// - `update_guidelines`: Trigger a re-index from the git repository
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
    GuidelineSearchResult, GuidelineSection as ApiGuidelineSection, GuidelineSummary,
    ListCategoryParams, SearchGuidelinesParams, SearchGuidelinesResponse, UpdateGuidelinesResponse,
};
use mcp_common::vectordb::VectorDb;

/// Shared application state, protected by `RwLock` for concurrent reads and
/// exclusive writes during re-indexing.
pub struct AppState {
    pub guidelines: HashMap<String, Guideline>,
    pub categories: HashMap<String, Category>,
}

#[derive(Clone)]
pub struct CppPerfGuidelinesServer {
    state: Arc<RwLock<AppState>>,
    search_engine: Arc<SearchEngine>,
    update_service: Arc<UpdateService>,
    cache: Arc<GuidelineCache>,
    tool_router: ToolRouter<CppPerfGuidelinesServer>,
}

impl CppPerfGuidelinesServer {
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
impl CppPerfGuidelinesServer {
    #[tool(
        description = "Search the low-level C++ performance guidelines (custom allocators, data layout, cache behavior, copy/move discipline, object lifetime, embedded constraints, concurrency memory effects, codegen) by semantic similarity. Returns ranked results."
    )]
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

    #[tool(
        description = "Get the full content of a specific C++ performance guideline by ID (e.g. 'MEM.1', 'CACHE.1', 'COPY.1')."
    )]
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

    #[tool(
        description = "List all C++ performance guidelines in a category. Categories: 'memory', 'copy-move', 'cache-layout', 'lifetime', 'embedded', 'concurrency', 'codegen'."
    )]
    async fn list_category(
        &self,
        Parameters(params): Parameters<ListCategoryParams>,
    ) -> Result<Json<CategoryListResponse>, String> {
        let category_key = params.category.trim().to_string();
        if category_key.is_empty() {
            return Err("category must not be empty".to_string());
        }

        let state = self.state.read().await;
        let (resolved_key, category) = state
            .categories
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(&category_key))
            .map(|(key, category)| (key.clone(), category.clone()))
            .ok_or_else(|| {
                let mut available: Vec<&str> =
                    state.categories.keys().map(|s| s.as_str()).collect();
                available.sort_unstable();
                format!(
                    "unknown category: '{category_key}'. Available categories: {}",
                    available.join(", ")
                )
            })?;

        let mut guideline_summaries: Vec<GuidelineSummary> = state
            .guidelines
            .values()
            .filter(|g| g.category == resolved_key)
            .map(|g| GuidelineSummary {
                id: g.id.clone(),
                title: g.title.clone(),
            })
            .collect();
        guideline_summaries.sort_by(|a, b| a.id.cmp(&b.id));

        let response = CategoryListResponse {
            category: CategoryInfo {
                key: category.key,
                display_name: category.display_name,
                guideline_count: category.guideline_count,
            },
            guidelines: guideline_summaries,
        };

        Ok(Json(response))
    }

    #[tool(
        description = "Trigger a re-index of the C++ performance guidelines from the git repository. Checks for updates and re-parses/re-embeds if the content has changed."
    )]
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
        sections: Some(
            guideline
                .sections
                .iter()
                .map(|s| ApiGuidelineSection {
                    heading: s.heading.clone(),
                    content: s.content.clone(),
                })
                .collect(),
        ),
        source_file: Some(guideline.source_file.clone()),
    }
}

#[tool_handler]
impl ServerHandler for CppPerfGuidelinesServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "cpp-perf-guidelines".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Low-level C++ performance guidelines MCP server. Curated technique-level \
                 guidance on custom allocators, data layout and cache behavior, copy/move \
                 discipline, object lifetime, embedded constraints, concurrency memory \
                 effects, and codegen — the layer below the ISO C++ Core Guidelines. Use \
                 search_guidelines for natural-language queries, get_guideline for lookup \
                 by ID (e.g. MEM.1), list_category to browse a category, and \
                 update_guidelines to refresh from the repository."
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CppPerfGuidelinesServer;

    #[test]
    fn tools_publish_output_schemas() {
        let tools = CppPerfGuidelinesServer::tool_router().list_all();
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
