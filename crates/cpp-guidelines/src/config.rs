use crate::error::AppError;

/// Application configuration loaded explicitly from environment variables.
///
/// No defaults are assumed for paths — the caller must provide them.
/// Redis URL is optional; if absent, the server runs without caching.
#[derive(Debug, Clone)]
pub struct Config {
    /// Redis connection URL (e.g. "redis://127.0.0.1:6379"). `None` disables caching.
    pub redis_url: Option<String>,
    /// Filesystem path to the LanceDB data directory.
    pub lancedb_path: String,
    /// Filesystem path to the cloned C++ Core Guidelines repository.
    pub repo_path: String,
    /// Whether `update_*` fetches and fast-forwards the clone before
    /// re-indexing. Disable for pinned or air-gapped deployments.
    pub repo_auto_pull: bool,
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// Required:
    /// - `LANCEDB_PATH`: path to LanceDB data directory
    /// - `CPP_GUIDELINES_REPO_AUTO_PULL`: set to 0/false/no/off to stop
    ///   `update_*` fetching from the remote (default: enabled)
    /// - `CPP_GUIDELINES_REPO_PATH`: path to the cloned guidelines repo
    ///
    /// Optional:
    /// - `REDIS_URL`: Redis connection string (omit to disable caching)
    pub fn from_env() -> Result<Self, AppError> {
        let lancedb_path = std::env::var("LANCEDB_PATH").map_err(|_| {
            AppError::Config("LANCEDB_PATH environment variable is required".to_string())
        })?;

        let repo_path = std::env::var("CPP_GUIDELINES_REPO_PATH").map_err(|_| {
            AppError::Config(
                "CPP_GUIDELINES_REPO_PATH environment variable is required".to_string(),
            )
        })?;

        // Validate that the repo path exists and contains the expected file
        let guidelines_file =
            std::path::Path::new(&repo_path).join("CppCoreGuidelines.md");
        if !guidelines_file.exists() {
            return Err(AppError::Config(format!(
                "CppCoreGuidelines.md not found at {}",
                guidelines_file.display()
            )));
        }

        let redis_url = std::env::var("REDIS_URL").ok();

        let repo_auto_pull =
            mcp_common::git::auto_pull_from_env("CPP_GUIDELINES_REPO_AUTO_PULL");

        Ok(Self {
            redis_url,
            lancedb_path,
            repo_path,
            repo_auto_pull,
        })
    }

    /// Returns the full path to the CppCoreGuidelines.md file.
    pub fn guidelines_file_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.repo_path).join("CppCoreGuidelines.md")
    }
}
