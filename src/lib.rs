//! Watch the status of the GitHub Action runs for a branch.
//!
//! The library part of the `watchgha` command; the binary is in `main.rs`.

pub mod bucketer;
pub mod data;
pub mod draw;
pub mod git;
pub mod http;
pub mod term;
pub mod urls;
pub mod utils;
pub mod watcher;
