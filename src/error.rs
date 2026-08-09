//! GAR CLI - error types
//!
//! Single source of truth for all errors in the `gar` CLI.
//! Maps to user-friendly messages and process exit codes.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum GarError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Command failed: {program} {args} (exit {code})\n{stderr}")]
    CommandFailed {
        program: String,
        args: String,
        code: i32,
        stderr: String,
    },

    #[error("Validation failed: {0}")]
    Validation(String),

    #[error("Lock held by another operation")]
    LockHeld,

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Runtime guard failed: {0}")]
    RuntimeGuard(String),

    #[error("Build failed: {0}")]
    Build(String),

    #[error("Publish failed: {0}")]
    Publish(String),

    #[error("Rollback failed: {0}")]
    Rollback(String),

    #[error("Garbage collection failed: {0}")]
    Gc(String),

    #[error("Doctor check failed: {0}")]
    Doctor(String),

    #[error("User management failed: {0}")]
    User(String),

    #[error("Group management failed: {0}")]
    Group(String),
}

impl GarError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    pub fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }

    pub fn runtime_guard(msg: impl Into<String>) -> Self {
        Self::RuntimeGuard(msg.into())
    }

    pub fn build(msg: impl Into<String>) -> Self {
        Self::Build(msg.into())
    }

    pub fn publish(msg: impl Into<String>) -> Self {
        Self::Publish(msg.into())
    }

    pub fn rollback(msg: impl Into<String>) -> Self {
        Self::Rollback(msg.into())
    }

    pub fn gc(msg: impl Into<String>) -> Self {
        Self::Gc(msg.into())
    }

    pub fn doctor(msg: impl Into<String>) -> Self {
        Self::Doctor(msg.into())
    }

    pub fn user(msg: impl Into<String>) -> Self {
        Self::User(msg.into())
    }

    pub fn group(msg: impl Into<String>) -> Self {
        Self::Group(msg.into())
    }

    /// Get process exit code for this error (Linux conventions).
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidArgument(_) => 2,
            Self::PermissionDenied(_) => 13,
            Self::CommandNotFound(_) => 127,
            Self::CommandFailed { code, .. } => *code,
            Self::LockHeld => 75,
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, GarError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_code_invalid_argument() {
        let e = GarError::invalid_argument("bad flag");
        assert_eq!(e.exit_code(), 2);
    }

    #[test]
    fn test_exit_code_permission_denied() {
        let e = GarError::PermissionDenied("root required".into());
        assert_eq!(e.exit_code(), 13);
    }

    #[test]
    fn test_exit_code_command_failed() {
        let e = GarError::CommandFailed {
            program: "nixos-rebuild".into(),
            args: "switch".into(),
            code: 25,
            stderr: "error".into(),
        };
        assert_eq!(e.exit_code(), 25);
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let gar_err: GarError = io_err.into();
        assert!(matches!(gar_err, GarError::Io(_)));
    }
}
