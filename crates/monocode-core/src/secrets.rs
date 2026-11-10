//! Where the app keeps its secrets (mail passwords and tokens, voice keys, a
//! sign-in's refresh token, the phone link's identity): the system's store,
//! under the app's internal ids. Every one goes through [`entry`].
//!
//! The system's store is used only once the app says so
//! ([`use_system_keychain`], first thing in its `main`). Until then, which is
//! always in a test, secrets live in this process's memory: tests never read
//! the user's own secrets, and the system never asks the user for them on a
//! test's behalf.
pub use monobot_email::keychain::{
    entry, use_system as use_system_keychain, uses_system as uses_system_keychain,
};
