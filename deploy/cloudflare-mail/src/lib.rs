//! monoBOT inbound mail Worker.
//!
//! Cloudflare Email Routing hands each message to `email`, which stores the
//! raw MIME in R2 and indexes it in D1. The desktop app pulls new messages over
//! HTTPS (`fetch`), and a daily cron (`scheduled`) applies retention. The
//! logic lives in plain Rust modules behind the `store` traits; `cf` binds them
//! to Cloudflare's runtime when compiled to wasm32.

pub mod auth_results;
pub mod config;
pub mod email;
pub mod http;
pub mod log;
pub mod retention;
pub mod store;

#[cfg(target_arch = "wasm32")]
mod cf;

#[cfg(test)]
mod tests;
