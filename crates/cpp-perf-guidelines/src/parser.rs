/// Parser for the cpp-perf-guidelines corpus.
///
/// The corpus has a deterministic structure (see the corpus repo README):
/// - `categories.toml` declares the category taxonomy.
/// - `guidelines/<category-key>/<ID>-<slug>.md` — one file per guideline.
/// - Each file: a TOML frontmatter block delimited by `+++`, then a Markdown body.
/// - Body sections are introduced by `## <Heading>` lines.
///
/// `categories.toml` failing to parse is a fatal, visible error — the whole
/// taxonomy is broken. An individual malformed guideline file is skipped with a
/// warning so one bad entry cannot prevent the server from starting.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;
use tracing::warn;

use crate::error::AppError;
use crate::model::{Category, Guideline, GuidelineSection};

/// Maximum characters of composed text fed to the embedding model.
const MAX_EMBEDDING_CHARS: usize = 2000;

#[derive(Debug, Deserialize)]
struct CategoriesFile {
    category: Vec<CategoryDecl>,
}

#[derive(Debug, Deserialize)]
struct CategoryDecl {
    key: String,
    token: String,
    display_name: String,
    order: u32,
    description: String,
}

#[derive(Debug, Deserialize)]
struct Frontmatter {
    id: String,
    title: String,
    category: String,
    status: String,
    summary: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// Parse the entire corpus rooted at `repo_path`.
///
/// Returns `(guidelines, categories)` where:
/// - `guidelines`: all successfully parsed guidelines, sorted by `id`
/// - `categories`: every category declared in `categories.toml`, keyed by `key`,
///   with `guideline_count` populated from the parsed guidelines
///
/// Fails only when `categories.toml` is missing or malformed.
pub fn parse_corpus(
    repo_path: &Path,
) -> Result<(Vec<Guideline>, HashMap<String, Category>), AppError> {
    // 1. Parse the category taxonomy. A failure here is fatal.
    let categories_path = repo_path.join("categories.toml");
    let categories_content = std::fs::read_to_string(&categories_path).map_err(|e| {
        AppError::Config(format!(
            "failed to read {}: {e}",
            categories_path.display()
        ))
    })?;
    let categories_file: CategoriesFile =
        toml::from_str(&categories_content).map_err(|e| AppError::Parse {
            file: "categories.toml".to_string(),
            message: e.to_string(),
        })?;

    let decls: HashMap<String, CategoryDecl> = categories_file
        .category
        .into_iter()
        .map(|c| (c.key.clone(), c))
        .collect();

    // 2. Collect every guideline markdown file under guidelines/<key>/.
    let guidelines_dir = repo_path.join("guidelines");
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    let dir_entries = std::fs::read_dir(&guidelines_dir).map_err(|e| {
        AppError::Config(format!(
            "failed to read {}: {e}",
            guidelines_dir.display()
        ))
    })?;
    for entry in dir_entries {
        let entry = entry.map_err(|e| AppError::Config(e.to_string()))?;
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let dir_category = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let file_entries =
            std::fs::read_dir(&dir_path).map_err(|e| AppError::Config(e.to_string()))?;
        for file in file_entries {
            let file = file.map_err(|e| AppError::Config(e.to_string()))?;
            let file_path = file.path();
            if file_path.extension().and_then(|e| e.to_str()) == Some("md") {
                files.push((dir_category.clone(), file_path));
            }
        }
    }
    // Sort for deterministic parse order regardless of filesystem iteration order.
    files.sort_by(|a, b| a.1.cmp(&b.1));

    // 3. Parse each file; skip malformed entries with a warning.
    let mut guidelines: Vec<Guideline> = Vec::new();
    for (dir_category, path) in &files {
        let source_file = path
            .strip_prefix(repo_path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!(file = %source_file, error = %e, "failed to read guideline file, skipping");
                continue;
            }
        };
        match parse_guideline_file(&content, &source_file, dir_category, &decls) {
            Ok(g) => guidelines.push(g),
            Err(e) => {
                warn!(file = %source_file, error = %e, "malformed guideline, skipping")
            }
        }
    }
    guidelines.sort_by(|a, b| a.id.cmp(&b.id));

    // Reject duplicate IDs — the ID is a stable, unique contract.
    let mut seen: HashSet<String> = HashSet::new();
    guidelines.retain(|g| {
        if seen.insert(g.id.clone()) {
            true
        } else {
            warn!(id = %g.id, "duplicate guideline id, skipping duplicate");
            false
        }
    });

    // 4. Build the category map — every declared category, with live counts.
    let mut counts: HashMap<String, usize> = HashMap::new();
    for g in &guidelines {
        *counts.entry(g.category.clone()).or_insert(0) += 1;
    }
    let mut categories: HashMap<String, Category> = HashMap::new();
    for (key, decl) in decls {
        let guideline_count = counts.get(&key).copied().unwrap_or(0);
        categories.insert(
            key,
            Category {
                key: decl.key,
                token: decl.token,
                display_name: decl.display_name,
                description: decl.description,
                order: decl.order,
                guideline_count,
            },
        );
    }

    Ok((guidelines, categories))
}

/// Parse a single guideline file. Returns `Err` with a human-readable reason
/// for any contract violation; the caller logs and skips.
fn parse_guideline_file(
    content: &str,
    source_file: &str,
    dir_category: &str,
    categories: &HashMap<String, CategoryDecl>,
) -> Result<Guideline, String> {
    let (frontmatter, body) = split_frontmatter(content)
        .ok_or_else(|| "missing or unterminated +++ frontmatter block".to_string())?;

    let fm: Frontmatter = toml::from_str(&frontmatter)
        .map_err(|e| format!("invalid frontmatter TOML: {e}"))?;

    if fm.status != "draft" && fm.status != "stable" {
        return Err(format!(
            "invalid status '{}', expected 'draft' or 'stable'",
            fm.status
        ));
    }

    let decl = categories
        .get(&fm.category)
        .ok_or_else(|| format!("unknown category '{}'", fm.category))?;

    if fm.category != dir_category {
        return Err(format!(
            "category '{}' does not match containing directory '{}'",
            fm.category, dir_category
        ));
    }

    let token = fm.id.split('.').next().unwrap_or_default();
    if token != decl.token {
        return Err(format!(
            "id '{}' has token '{token}' but category '{}' uses token '{}'",
            fm.id, fm.category, decl.token
        ));
    }

    let anchor = derive_anchor(source_file, &fm.id);
    let sections = parse_sections(&body);

    Ok(Guideline {
        id: fm.id,
        anchor,
        title: fm.title,
        category: fm.category,
        status: fm.status,
        tags: fm.tags,
        summary: fm.summary,
        sections,
        raw_markdown: body.trim().to_string(),
        source_file: source_file.to_string(),
    })
}

/// Split a file into its `+++`-delimited TOML frontmatter and Markdown body.
///
/// The file must begin with a line containing exactly `+++` and contain a
/// matching closing `+++`. Returns `None` if either delimiter is absent.
fn split_frontmatter(content: &str) -> Option<(String, String)> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "+++" {
        return None;
    }
    let mut frontmatter: Vec<&str> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let mut closed = false;
    for line in lines {
        if !closed && line.trim() == "+++" {
            closed = true;
            continue;
        }
        if closed {
            body.push(line);
        } else {
            frontmatter.push(line);
        }
    }
    if !closed {
        return None;
    }
    Some((frontmatter.join("\n"), body.join("\n")))
}

/// Split a Markdown body into `## <Heading>` sections.
///
/// `###` (and deeper) headings are treated as section content, not new sections.
fn parse_sections(body: &str) -> Vec<GuidelineSection> {
    let mut sections: Vec<GuidelineSection> = Vec::new();
    let mut heading: Option<String> = None;
    let mut buffer: Vec<&str> = Vec::new();

    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(prev) = heading.take() {
                sections.push(GuidelineSection {
                    heading: prev,
                    content: buffer.join("\n").trim().to_string(),
                });
            }
            heading = Some(rest.trim().to_string());
            buffer.clear();
        } else {
            buffer.push(line);
        }
    }
    if let Some(prev) = heading.take() {
        sections.push(GuidelineSection {
            heading: prev,
            content: buffer.join("\n").trim().to_string(),
        });
    }
    sections
}

/// Derive the guideline `anchor` from its filename.
///
/// Filenames follow `<ID>-<slug>.md`; the anchor is `<slug>`. If the filename
/// does not start with `<ID>-`, the full stem is used and a warning is logged.
fn derive_anchor(source_file: &str, id: &str) -> String {
    let stem = source_file
        .rsplit('/')
        .next()
        .unwrap_or(source_file)
        .strip_suffix(".md")
        .unwrap_or(source_file);
    match stem.strip_prefix(&format!("{id}-")) {
        Some(slug) => slug.to_string(),
        None => {
            warn!(
                file = %source_file,
                id,
                "filename does not match '<ID>-<slug>.md'; using full stem as anchor"
            );
            stem.to_string()
        }
    }
}

/// Compose the text embedded for semantic search.
///
/// Concatenates title, summary, and the Rationale and Guidance sections — the
/// parts that carry the guideline's meaning. Truncated to a bounded length.
pub fn compose_embedding_text(guideline: &Guideline) -> String {
    let mut parts = vec![guideline.title.clone(), guideline.summary.clone()];
    for section in &guideline.sections {
        if section.heading == "Rationale" || section.heading == "Guidance" {
            parts.push(section.content.clone());
        }
    }
    let text = parts.join(". ");
    if text.chars().count() > MAX_EMBEDDING_CHARS {
        text.chars().take(MAX_EMBEDDING_CHARS).collect()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decls() -> HashMap<String, CategoryDecl> {
        let mut m = HashMap::new();
        m.insert(
            "memory".to_string(),
            CategoryDecl {
                key: "memory".to_string(),
                token: "MEM".to_string(),
                display_name: "Custom Allocators & Memory Management".to_string(),
                order: 1,
                description: "Allocators.".to_string(),
            },
        );
        m
    }

    const SAMPLE: &str = r#"+++
id = "MEM.1"
title = "Use an arena allocator"
category = "memory"
status = "draft"
summary = "Bump a pointer, reset all at once."
tags = ["arena", "allocator"]
+++

## Rationale

General-purpose malloc has unpredictable cost.

## Guidance

Reserve a buffer once; advance an offset.

### A subheading

Stays inside Guidance.

## Example

    void* p = arena.allocate(64, 8);
"#;

    #[test]
    fn splits_frontmatter_and_body() {
        let (fm, body) = split_frontmatter(SAMPLE).expect("frontmatter present");
        assert!(fm.contains("id = \"MEM.1\""));
        assert!(body.trim_start().starts_with("## Rationale"));
    }

    #[test]
    fn missing_frontmatter_returns_none() {
        assert!(split_frontmatter("## No frontmatter here\n").is_none());
        assert!(split_frontmatter("+++\nid = \"X\"\nno closing delimiter\n").is_none());
    }

    #[test]
    fn parses_sections_and_keeps_subheadings_as_content() {
        let (_, body) = split_frontmatter(SAMPLE).unwrap();
        let sections = parse_sections(&body);
        let headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["Rationale", "Guidance", "Example"]);
        let guidance = sections.iter().find(|s| s.heading == "Guidance").unwrap();
        assert!(guidance.content.contains("### A subheading"));
    }

    #[test]
    fn parses_a_well_formed_guideline() {
        let g = parse_guideline_file(SAMPLE, "guidelines/memory/MEM.1-arena.md", "memory", &decls())
            .expect("valid guideline");
        assert_eq!(g.id, "MEM.1");
        assert_eq!(g.anchor, "arena");
        assert_eq!(g.category, "memory");
        assert_eq!(g.status, "draft");
        assert_eq!(g.tags, ["arena", "allocator"]);
        assert_eq!(g.sections.len(), 3);
    }

    #[test]
    fn rejects_token_category_mismatch() {
        let bad = SAMPLE.replace(r#"id = "MEM.1""#, r#"id = "CACHE.1""#);
        let err = parse_guideline_file(&bad, "f.md", "memory", &decls()).unwrap_err();
        assert!(err.contains("token"), "got: {err}");
    }

    #[test]
    fn rejects_directory_mismatch() {
        let err =
            parse_guideline_file(SAMPLE, "f.md", "cache-layout", &decls()).unwrap_err();
        assert!(err.contains("does not match containing directory"), "got: {err}");
    }

    #[test]
    fn rejects_invalid_status() {
        let bad = SAMPLE.replace(r#"status = "draft""#, r#"status = "published""#);
        let err = parse_guideline_file(&bad, "f.md", "memory", &decls()).unwrap_err();
        assert!(err.contains("invalid status"), "got: {err}");
    }

    #[test]
    fn composes_embedding_text_from_meaningful_parts() {
        let g = parse_guideline_file(SAMPLE, "guidelines/memory/MEM.1-arena.md", "memory", &decls())
            .unwrap();
        let text = compose_embedding_text(&g);
        assert!(text.starts_with("Use an arena allocator"));
        assert!(text.contains("unpredictable cost"));
        assert!(text.contains("advance an offset"));
    }

    /// Integration test: parse the real corpus when its path is available.
    #[test]
    fn parses_real_corpus_when_present() {
        let repo = std::env::var("CPP_PERF_GUIDELINES_REPO_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("../../data/cpp-perf-guidelines"));
        if !repo.join("categories.toml").exists() {
            eprintln!("skipping: corpus not found at {}", repo.display());
            return;
        }
        let (guidelines, categories) = parse_corpus(&repo).expect("corpus parses");
        assert_eq!(categories.len(), 8, "expected 8 declared categories");
        for g in &guidelines {
            assert!(
                categories.contains_key(&g.category),
                "guideline {} has undeclared category {}",
                g.id,
                g.category
            );
        }
        eprintln!(
            "parsed {} guidelines across {} categories",
            guidelines.len(),
            categories.len()
        );
    }
}
