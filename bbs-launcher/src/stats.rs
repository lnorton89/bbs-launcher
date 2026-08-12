use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemStats {
    #[serde(default)]
    pub count: u64,
    pub last_launched: Option<u64>,
}

/// Per-item launch history, persisted across sessions so the menu can show
/// usage counts and "last run" times.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    #[serde(default)]
    pub items: HashMap<String, ItemStats>,
}

impl Stats {
    pub fn path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config").join("bbs-launcher").join("stats.toml"))
    }

    /// Any failure (missing file, bad TOML) just starts fresh — stats are
    /// nice-to-have, never worth blocking launch over.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path().context("no home directory for stats file")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn record(&mut self, label: &str) {
        let entry = self.items.entry(label.to_string()).or_default();
        entry.count += 1;
        entry.last_launched = Some(now_secs());
    }

    pub fn get(&self, label: &str) -> Option<&ItemStats> {
        self.items.get(label)
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn time_ago(then: u64) -> String {
    let d = now_secs().saturating_sub(then);
    match d {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86400),
    }
}
