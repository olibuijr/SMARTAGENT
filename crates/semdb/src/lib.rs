//! semdb — pure-Rust, std-only semantic database.
//! Log-structured single-file store + in-memory HNSW, external embeddings.

pub mod cli;
pub mod config;
pub mod hnsw;
pub mod http;
/// JSON is the shared `httpc::json` implementation, re-exported so there is a
/// single `Value` type across the workspace (was duplicated byte-for-byte).
pub use httpc::json;
pub mod storage;
pub mod vector;
pub mod workspace;
