use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct BbsItem {
    pub key: String,
    pub label: String,
    /// Command to run. Omitted for "complex" items that open a built-in
    /// screen instead (`screen`).
    #[serde(default)]
    pub cmd: String,
    pub desc: String,
    pub icon: String,
    pub color: String,
    pub wsl: Option<bool>,
    /// Optional section this item is grouped under in the menu. Items
    /// without a category are listed last, ungrouped.
    pub category: Option<String>,
    /// Working directory to launch the command from.
    pub cwd: Option<String>,
    /// Wait for Enter after the command exits before returning to the menu.
    /// Useful for short commands like `git status` whose output would
    /// otherwise vanish immediately.
    pub pause: Option<bool>,
    /// Built-in screen this item opens instead of launching a command.
    /// Supported values: "github" (all-in-one dashboard).
    #[serde(default)]
    pub screen: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BbsConfig {
    pub bbs: BbsHeader,
    pub items: Vec<BbsItem>,
    /// GitHub dashboard customization. Omit for sensible defaults.
    #[serde(default)]
    pub github: Option<GithubConfig>,
}

/// Customization for the built-in GitHub dashboard screen.
/// `PartialEq` so a live config reload can tell whether this section
/// changed and the dashboard needs rebuilding.
#[derive(Debug, Deserialize, Clone, Default, PartialEq)]
pub struct GithubConfig {
    /// Sections to show, in display order. Defaults to all six:
    /// `notifications`, `pull_requests`, `issues`, `stars`, `gists`,
    /// `profile`. Unknown names are ignored.
    #[serde(default)]
    pub sections: Option<Vec<String>>,
    /// Max entries fetched per section (clamped to 1..=100). Default 25.
    #[serde(default)]
    pub per_page: Option<usize>,
    /// Auto-refresh interval in seconds while the screen is open
    /// (minimum 5). Default 120.
    #[serde(default)]
    pub refresh_secs: Option<u64>,
    /// Repo affiliation filter for the Issues and Pull Requests sections:
    /// comma-separated subset of `owner`, `collaborator` (write access),
    /// `organization_member`. Default: all three.
    #[serde(default)]
    pub affiliation: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BbsHeader {
    pub title: String,
    /// Shown centered under the banner art.
    pub subtitle: Option<String>,
    /// Banner font: "shadow" (default, solid fill) or "lined" (same
    /// letterforms, horizontal-line fill instead of solid). See the
    /// `blockfont` crate for the full set of accepted names.
    pub banner_style: Option<String>,
    /// Accent color for the banner, borders, and selection highlight.
    /// Any of the standard color names (cyan, magenta, green, ...).
    /// Defaults to cyan.
    pub theme: Option<String>,
    /// Animate the banner with a slow color shimmer. Defaults to true.
    pub banner_animation: Option<bool>,
    /// Run a travelling light around every pane border. Under `rainbow`
    /// it sweeps the hue wheel; under a solid theme it chases a dim band
    /// through the theme colour. Defaults to true; needs
    /// `banner_animation` on to actually move.
    pub border_chase: Option<bool>,
    /// Seconds for the border chase to travel one full lap of a pane.
    /// Lower is faster. Defaults to 12; values outside 0.5-600 are
    /// clamped.
    pub chase_lap_secs: Option<f32>,
    /// Message-of-the-day lines, shown as a scrolling ticker under the
    /// banner. Omit (or leave empty) to hide the ticker entirely.
    pub motd: Option<Vec<String>>,
}

pub fn load_config(override_path: Option<PathBuf>) -> Result<(BbsConfig, PathBuf)> {
    let config_path = match override_path {
        Some(p) => p,
        None => find_config()?,
    };
    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from: {}", config_path.display()))?;
    let config: BbsConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse TOML config: {}", config_path.display()))?;
    Ok((config, config_path))
}

pub fn find_config() -> Result<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(exe_dir) = std::env::current_exe() {
        if let Some(parent) = exe_dir.parent() {
            paths.push(parent.join("bbs.toml"));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("bbs.toml"));
    }

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".config").join("bbs-launcher").join("bbs.toml"));
    }

    for path in &paths {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    Ok(paths.into_iter().next().unwrap())
}
