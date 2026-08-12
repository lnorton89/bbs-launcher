use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct BbsItem {
    pub key: String,
    pub label: String,
    pub cmd: String,
    pub desc: String,
    pub icon: String,
    pub color: String,
    pub wsl: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BbsConfig {
    pub bbs: BbsHeader,
    pub items: Vec<BbsItem>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BbsHeader {
    pub title: String,
    /// Banner font: "shadow" (default, solid fill) or "lined" (same
    /// letterforms, horizontal-line fill instead of solid). See the
    /// `blockfont` crate for the full set of accepted names.
    pub banner_style: Option<String>,
}

pub fn load_config() -> Result<BbsConfig> {
    let config_path = find_config()?;
    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config from: {}", config_path.display()))?;
    let config: BbsConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse TOML config: {}", config_path.display()))?;
    Ok(config)
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
