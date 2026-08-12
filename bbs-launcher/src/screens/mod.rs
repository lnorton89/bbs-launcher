//! Built-in full-screen views the menu can open instead of launching a
//! command (`screen = "..."` on an item). Each screen owns its state
//! (`*View`), fetches on background threads, and handles its own keys.

pub mod bluetti;
pub mod github;

/// Result of handling one key while a screen is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Stay,
    Back,
}
