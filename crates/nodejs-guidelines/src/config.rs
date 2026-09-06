use std::path::{Path, PathBuf};

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Config {
    pub redis_url: Option<String>,
    pub lancedb_path: String,
    pub repo_path: String,
    /// Whether `update_*` fetches and fast-forwards the clone before
    /// re-indexing. Disable for pinned or air-gapped deployments.
    pub repo_auto_pull: bool,
    pub readme_rel_path: String,
}

impl Config {
    /// Required:
    /// - `LANCEDB_PATH`
    /// - `NODEJS_GUIDELINES_REPO_PATH` (path to the cloned nodebestpractices repo)
    ///
    /// Optional:
    /// - `REDIS_URL`
    /// - `NODEJS_GUIDELINES_README` (default: "README.md")
    pub fn from_env() -> Result<Self, AppError> {
        let lancedb_path = std::env::var("LANCEDB_PATH")
            .map_err(|_| AppError::Config("LANCEDB_PATH environment variable is required".to_string()))?;

        let repo_path = std::env::var("NODEJS_GUIDELINES_REPO_PATH").map_err(|_| {
            AppError::Config(
                "NODEJS_GUIDELINES_REPO_PATH environment variable is required".to_string(),
            )
        })?;

        let readme_rel_path =
            std::env::var("NODEJS_GUIDELINES_README").unwrap_or_else(|_| "README.md".to_string());

        let mut resolved_repo_path = repo_path.clone();
        let readme = Path::new(&resolved_repo_path).join(&readme_rel_path);
        if !readme.exists() {
            let nested_repo = Path::new(&repo_path).join("nodebestpractices");
            let nested_readme = nested_repo.join(&readme_rel_path);
            if nested_readme.exists() {
                resolved_repo_path = nested_repo.to_string_lossy().to_string();
            } else {
                return Err(AppError::Config(format!(
                    "required file not found: {} (also checked {})",
                    readme.display(),
                    nested_readme.display()
                )));
            }
        }

        let repo_auto_pull =
            mcp_common::git::auto_pull_from_env("NODEJS_GUIDELINES_REPO_AUTO_PULL");

        Ok(Self {
            redis_url: std::env::var("REDIS_URL").ok(),
            lancedb_path,
            repo_path: resolved_repo_path,
            repo_auto_pull,
            readme_rel_path,
        })
    }

    pub fn repo_path(&self) -> PathBuf {
        Path::new(&self.repo_path).to_path_buf()
    }

    pub fn guidelines_file_path(&self) -> PathBuf {
        Path::new(&self.repo_path).join(&self.readme_rel_path)
    }
}
