//! Agent visuals for the Tauri app. The interpreter, its SDK and the component
//! tree are the native app's own files, compiled here so both apps run one
//! sandbox and accept the same programs.
#[path = "../../monocode-core/src/visual.rs"]
mod visual;
pub use visual::*;

/// The SDK reference an agent reads before it writes a visual.
pub const DOCUMENTATION: &str = include_str!("../../monocode-core/src/visual/docs.md");
/// When an agent should reach for a visual, for its instructions.
pub const USAGE: &str = include_str!("../../monocode-core/src/visual/usage.md");
