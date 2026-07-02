//! rag — document ingestion and cited chunk retrieval.
//!
//! A lean RAGFlow-pattern port: parse document text, split it into stable
//! chunks, embed chunks via an external endpoint, store them in semdb, and
//! retrieve chunks with citation metadata.

pub mod chunk;
pub mod cli;
pub mod embed;
pub mod pdftext;
pub mod store;
