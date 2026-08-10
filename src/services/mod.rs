//! GAR CLI - shared services.
//!
//! Modules that wrap system-level operations (git, nix, btrfs, etc).
//! Each service module owns a thin wrapper around one external tool
//! so commands can stay focused on orchestration logic.

pub mod atomic_file;
pub mod branding;
pub mod btrfs;
pub mod build;
pub mod channel;
pub mod client;
pub mod filesystem;
pub mod generation;
pub mod generations;
pub mod git;
pub mod group_system;
pub mod lock;
pub mod manifest;
pub mod nix;
pub mod nixos_rebuild;
pub mod rollback;
pub mod runtime_guard;
pub mod shell;
pub mod user_system;
