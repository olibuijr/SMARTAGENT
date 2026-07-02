//! secrets — policy-gated, audited secret store (Infisical/Vaultwarden concept).
//! All data lives in semdb tables (store/policy/audit). Values obfuscated at
//! rest with ChaCha20-Poly1305 AEAD (see aead.rs / store.rs).
pub mod aead;
pub mod audit;
pub mod cli;
pub mod policy;
pub mod store;
pub mod token;
