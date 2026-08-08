//! tactus — headless orchestration engine for AI coding agents.
//!
//! Step 1 scope: `tactus validate` only. Parse an annotated markdown plan
//! into the IR, load optional config, resolve routing chains with a binder
//! preview, and report — executing nothing.

pub mod agent;
pub mod catalog;
pub mod config;
pub mod engine;
pub mod error;
pub mod gates;
pub mod ir;
pub mod plan;
pub mod route;
pub mod ulid;
pub mod validate;
pub mod workspace;
