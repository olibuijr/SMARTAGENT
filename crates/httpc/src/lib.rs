//! httpc — zero-crates HTTP/1.1 client library shared by all SMARTAGENT crates.
//! HTTP uses `std::net::TcpStream`; HTTPS uses the system `openssl s_client`
//! helper and the same request/response parser.

pub mod args;
pub mod client;
pub mod json;
pub mod url;

pub use client::{get, post_json, request, Request, Response};
pub use url::Url;
