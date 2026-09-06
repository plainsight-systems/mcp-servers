use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchGuidelinesParams {
    /// The search query describing what you're looking for.
    pub query: String,
    /// Maximum number of results to return (default: 10, max: 50).
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetGuidelineParams {
    /// Stable guideline ID such as "P.1" or "C-CASE".
    pub guideline_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListCategoryParams {
    /// Category key/prefix such as "ES" or "Naming".
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuidelineSearchResult {
    pub id: String,
    pub title: String,
    pub category: String,
    pub score: f32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchGuidelinesResponse {
    pub results: Vec<GuidelineSearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuidelineSection {
    pub heading: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuidelineDetailResponse {
    pub id: String,
    pub anchor: String,
    pub title: String,
    pub category: String,
    pub raw_markdown: String,
    /// Populated when a source has explicit subsection structure (for example C++ guidelines).
    pub sections: Option<Vec<GuidelineSection>>,
    /// Populated when a source is chapter/file based (for example Rust API guidelines).
    pub source_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CategoryInfo {
    pub key: String,
    pub display_name: String,
    pub guideline_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GuidelineSummary {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CategoryListResponse {
    pub category: CategoryInfo,
    pub guidelines: Vec<GuidelineSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateGuidelinesResponse {
    /// Whether a re-index occurred.
    pub updated: bool,
    /// The commit the index now reflects.
    pub commit: String,
    pub guideline_count: usize,
    /// What happened at the remote before the commit check: fast-forwarded,
    /// already current, disabled, skipped, or a stated failure.
    ///
    /// Present so a caller can distinguish "already up to date" from "never
    /// contacted the remote". Without it, a server that cannot reach origin
    /// reports success indistinguishable from a genuine no-op.
    pub remote_sync: String,
}
