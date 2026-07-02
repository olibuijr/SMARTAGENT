//! semdb — pure-Rust, std-only semantic database.
//! Log-structured single-file store + in-memory HNSW, external embeddings.

pub mod cli;
pub mod hnsw;
pub mod http;
pub mod json;
pub mod storage;
pub mod vector;
