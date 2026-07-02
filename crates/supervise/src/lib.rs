//! supervise — a tiny pure-Rust process manager for SMARTAGENT's long-running
//! services (the scheduler daemon, headless Chromium). It spawns them detached,
//! tracks pid + command + start time in a semdb table, health-checks by /proc
//! liveness (+ optional HTTP probe), and self-heals dead services in a watch
//! loop. No systemd required; reboot-persistence is one optional `@reboot`
//! crontab line running `supervise watch`.
//!
//! Why not systemd: keeping process control inside the agent means the `supervise`
//! tool can start/stop/restart/status services directly, everything stays
//! zero-dep pure Rust, and state lives in a semdb table like all other data.

pub mod proc;
pub mod services;
pub mod state;
pub mod cli;
