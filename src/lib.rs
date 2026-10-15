//! GAR library — re-exports the same modules the `gar` binary uses.
//!
//! Splitting `src/lib.rs` from `src/main.rs` makes the codebase
//! reusable: integration tests under `tests/` and future
//! `gar-*` sub-binaries (e.g. `gar-server`, `gar-client`) can
//! `use gar::services::discovery;` instead of depending on the
//! binary crate layout.

pub mod cli;
pub mod commands;
pub mod config;
pub mod error;
pub mod output;
pub mod services;
