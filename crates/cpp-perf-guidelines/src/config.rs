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
    /// Filesystem path to the cloned cpp-perf-guidelines corpus repository.
    pub repo_path: String,
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// Required:
    /// - `LANCEDB_PATH`: path to LanceDB data directory
    /// - `CPP_PERF_GUIDELINES_REPO_PATH`: path to the cloned corpus repo
    ///
    /// Optional:
    /// - `REDIS_URL`: Redis connection string (omit to disable caching)
    pub fn from_env() -> Result<Self, AppError> {
        let lancedb_path = std::env::var("LANCEDB_PATH").map_err(|_| {
            AppError::Config("LANCEDB_PATH environment variable is required".to_string())
        })?;

        let repo_path = std::env::var("CPP_PERF_GUIDELINES_REPO_PATH").map_err(|_| {
            AppError::Config(
                "CPP_PERF_GUIDELINES_REPO_PATH environment variable is required".to_string(),
            )
        })?;

        // Validate the corpus repo layout: categories.toml + guidelines/ must exist.
        let categories_file = std::path::Path::new(&repo_path).join("categories.toml");
        if !categories_file.exists() {
            return Err(AppError::Config(format!(
                "categories.toml not found at {}",
                categories_file.display()
            )));
        }
        let guidelines_dir = std::path::Path::new(&repo_path).join("guidelines");
        if !guidelines_dir.is_dir() {
            return Err(AppError::Config(format!(
                "guidelines/ directory not found at {}",
                guidelines_dir.display()
            )));
        }

        let redis_url = std::env::var("REDIS_URL").ok();

        Ok(Self {
            redis_url,
            lancedb_path,
            repo_path,
        })
    }

    /// Returns the corpus repo root as a `Path`.
    pub fn repo_dir(&self) -> &std::path::Path {
        std::path::Path::new(&self.repo_path)
    }
}
