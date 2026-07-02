//! secrets — policy-gated, audited secret store (Infisical/Vaultwarden concept).
//! All data lives in semdb tables (store/policy/audit). Values obfuscated at
//! rest (NOT strong crypto — see store.rs).
pub mod audit;
pub mod cli;
pub mod policy;
pub mod store;
