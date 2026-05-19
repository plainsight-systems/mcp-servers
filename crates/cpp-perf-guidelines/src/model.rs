use serde::{Deserialize, Serialize};

/// A single low-level C++ performance guideline (e.g. "MEM.1").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guideline {
    /// Stable identifier, `<TOKEN>.<n>` — e.g. "MEM.1", "CACHE.1".
    pub id: String,
    /// Filename slug (the `<slug>` portion of `<ID>-<slug>.md`).
    pub anchor: String,
    /// Short imperative title.
    pub title: String,
    /// Category key — e.g. "memory", "cache-layout".
    pub category: String,
    /// Lifecycle status — "draft" or "stable".
    pub status: String,
    /// Cross-cutting tags.
    pub tags: Vec<String>,
    /// One-line summary, surfaced in search results.
    pub summary: String,
    /// Body sections keyed by `##` heading (Rationale, Guidance, Example, ...).
    pub sections: Vec<GuidelineSection>,
    /// Markdown body of the guideline (frontmatter excluded).
    pub raw_markdown: String,
    /// Path of the source file relative to the corpus repo root.
    pub source_file: String,
}

/// A `##`-delimited body section within a guideline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidelineSection {
    /// Section heading, e.g. "Rationale", "Guidance", "Example".
    pub heading: String,
    /// Section content (markdown).
    pub content: String,
}

/// A search result returned from vector similarity search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidelineResult {
    pub id: String,
    pub title: String,
    pub category: String,
    /// Similarity score in [0, 1]; higher is more similar.
    pub score: f32,
    pub summary: String,
}

/// A guideline category, declared in `categories.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    /// Directory name and `category:` frontmatter value — e.g. "memory".
    pub key: String,
    /// Uppercase ID prefix — e.g. "MEM".
    pub token: String,
    /// Human-readable name.
    pub display_name: String,
    /// One-line scope statement.
    pub description: String,
    /// Display / documentation order.
    pub order: u32,
    /// Number of guidelines parsed into this category.
    pub guideline_count: usize,
}
