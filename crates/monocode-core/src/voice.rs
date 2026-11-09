//! Voice beyond the local models: the cloud providers a call can speak
//! through and their API keys. The keys live in the system keychain, never in
//! settings files, process arguments or logs.
pub mod keys;

pub use keys::Provider;
