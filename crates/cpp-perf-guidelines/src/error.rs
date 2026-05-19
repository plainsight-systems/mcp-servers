use mcp_common::error::CommonError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Common(#[from] CommonError),

    #[error("parse error in {file}: {message}")]
    Parse { file: String, message: String },

    #[error("git error: {0}")]
    Git(String),

    #[error("config error: {0}")]
    Config(String),
}
