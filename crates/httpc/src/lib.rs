//! httpc — pure-Rust, std-only HTTP/1.1 client library shared by all
//! SMARTAGENT crates. Plain HTTP only; https egress routes via a local proxy.

pub mod args;
pub mod client;
pub mod json;
pub mod url;

pub use client::{get, post_json, request, Request, Response};
pub use url::Url;
