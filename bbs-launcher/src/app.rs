use crate::config::{BbsConfig, BbsItem};
use crate::github::GithubView;
use crate::stats::Stats;
use crate::ui::{color_from_str, hsv_to_rgb};
use ratatui::{layout::Rect, style::Color, widgets::ListState};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

/// UI ticks per second. Animation phases are derived from the tick
/// counter, so this is what converts configured durations into the
/// per-tick steps the drawing code works in.
pub const TICKS_PER_SEC: f32 = 10.0;

/// Seconds for one lap of the rainbow border chase when unconfigured.
const DEFAULT_CHASE_LAP_SECS: f32 = 12.0;

/// Fast enough to still read as a chase, slow enough not to strobe.
const MIN_CHASE_LAP_SECS: f32 = 0.5;
const MAX_CHASE_LAP_SECS: f32 = 600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    Help,
    /// Built-in GitHub dashboard screen.
    Github,
}

/// Accent theme. `Solid` uses one named color everywhere; `Rainbow`
/// animates the accent through the full hue wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Solid(Color),
    Rainbow,
}

impl Theme {
    pub fn parse(s: &str) -> Theme {
        match s.to_ascii_lowercase().as_str() {
            "rainbow" | "pride" => Theme::Rainbow,
            other => Theme::Solid(color_from_str(other)),
        }
    }
}

/// One visible line of the menu: a collapsible category header, or an
/// actual launchable item (index into `App::items`).
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Header { name: String, count: usize },
    Item(usize),
}

#[derive(Debug)]
pub struct App {
    pub config: BbsConfig,
    pub items: Vec<BbsItem>,
    pub state: ListState,
    pub status_message: String,
    pub spinner: usize,
    pub banner: String,
    pub mode: Mode,
    pub query: String,
    /// Indices into `items` that match the current search query.
    pub filtered: Vec<usize>,
    /// What the menu actually shows: headers + visible items, derived
    /// from `filtered` and `collapsed`. `state` selects within this.
    pub rows: Vec<Row>,
    pub collapsed: HashSet<String>,
    pub stats: Stats,
    pub tick: u64,
    pub theme: Theme,
    /// Whether the banner/accent animates (config `banner_animation`).
    pub animate: bool,
    /// Whether the travelling border light runs at all.
    pub chase: bool,
    /// Degrees the border chase advances per tick, derived from the
    /// configured lap time.
    pub chase_degrees_per_tick: f32,
    /// Where the menu was last drawn, for mouse hit-testing.
    pub menu_area: Option<Rect>,
    pub last_click: Option<(Instant, usize)>,
    pub session_start: Instant,
    pub config_path: String,
    /// Pre-joined message-of-the-day text for the scrolling ticker.
    /// `None` when no `motd` is configured, which hides the row entirely.
    pub motd: Option<String>,
    /// State for the built-in GitHub dashboard screen.
    pub github: GithubView,
}

impl App {
    pub fn new(config: BbsConfig, config_path: PathBuf) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        let style = config
            .bbs
            .banner_style
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        let banner = blockfont::render(&get_hostname(), style);
        let theme = config
            .bbs
            .theme
            .as_deref()
            .map(Theme::parse)
            .unwrap_or(Theme::Solid(Color::Cyan));
        let animate = config.bbs.banner_animation.unwrap_or(true);
        // Reject NaN/infinity before clamping, so a nonsense value falls
        // back to the default rather than freezing the chase.
        let chase_lap_secs = config
            .bbs
            .chase_lap_secs
            .filter(|s| s.is_finite())
            .map(|s| s.clamp(MIN_CHASE_LAP_SECS, MAX_CHASE_LAP_SECS))
            .unwrap_or(DEFAULT_CHASE_LAP_SECS);
        let chase_degrees_per_tick = 360.0 / (chase_lap_secs * TICKS_PER_SEC);
        let chase = config.bbs.border_chase.unwrap_or(true);
        let github = GithubView::new(config.github.clone());
        // Blank/whitespace-only entries are dropped so a stray empty
        // string in the config can't produce a row of dead space.
        let motd = config.bbs.motd.as_ref().and_then(|lines| {
            let joined = lines
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("   ✦   ");
            (!joined.is_empty()).then(|| format!("{joined}   ✦   "))
        });
        let items = config.items.clone();
        let filtered = (0..items.len()).collect();
        let mut app = Self {
            config,
            items,
            state,
            status_message: "Ready".to_string(),
            spinner: 0,
            banner,
            mode: Mode::Normal,
            query: String::new(),
            filtered,
            rows: Vec::new(),
            collapsed: HashSet::new(),
            stats: Stats::load(),
            tick: 0,
            theme,
            animate,
            chase,
            chase_degrees_per_tick,
            menu_area: None,
            last_click: None,
            session_start: Instant::now(),
            config_path: config_path.display().to_string(),
            motd,
            github,
        };
        app.rebuild_rows();
        app
    }

    /// The accent color this frame. Solid themes are constant; the
    /// rainbow theme walks the hue wheel as the app ticks.
    pub fn accent(&self) -> Color {
        match self.theme {
            Theme::Solid(c) => c,
            Theme::Rainbow => {
                let hue = if self.animate {
                    (self.tick as f32 * 2.4) % 360.0
                } else {
                    200.0
                };
                let (r, g, b) = hsv_to_rgb(hue, 0.7, 1.0);
                Color::Rgb(r, g, b)
            }
        }
    }

    pub fn next(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) if i + 1 < self.rows.len() => i + 1,
            _ => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(0) | None => self.rows.len() - 1,
            Some(i) => i - 1,
        };
        self.state.select(Some(i));
    }

    pub fn select_first(&mut self) {
        if !self.rows.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub fn select_last(&mut self) {
        if !self.rows.is_empty() {
            self.state.select(Some(self.rows.len() - 1));
        }
    }

    pub fn jump(&mut self, delta: i64) {
        if self.rows.is_empty() {
            return;
        }
        let len = self.rows.len() as i64;
        let cur = self.state.selected().unwrap_or(0) as i64;
        let next = (cur + delta).clamp(0, len - 1);
        self.state.select(Some(next as usize));
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.state.selected().and_then(|i| self.rows.get(i))
    }

    pub fn selected_item(&self) -> Option<&BbsItem> {
        match self.selected_row() {
            Some(Row::Item(idx)) => self.items.get(*idx),
            _ => None,
        }
    }

    pub fn find_by_key(&self, key: &str) -> Option<&BbsItem> {
        self.items
            .iter()
            .find(|item| item.key.eq_ignore_ascii_case(key))
    }

    /// Re-select a specific item (by label) after the rows change.
    /// No-op if the item isn't currently visible.
    pub fn select_label(&mut self, label: &str) {
        if let Some(pos) = self
            .rows
            .iter()
            .position(|r| matches!(r, Row::Item(i) if self.items[*i].label == label))
        {
            self.state.select(Some(pos));
        }
    }

    fn select_header(&mut self, name: &str) {
        if let Some(pos) = self
            .rows
            .iter()
            .position(|r| matches!(r, Row::Header { name: n, .. } if n == name))
        {
            self.state.select(Some(pos));
        }
    }

    /// Folds or unfolds the header at `row`, keeping it selected.
    /// Returns false when that row isn't a header.
    pub fn toggle_category_at(&mut self, row: usize) -> bool {
        let Some(Row::Header { name, .. }) = self.rows.get(row) else {
            return false;
        };
        let name = name.clone();
        if !self.collapsed.remove(&name) {
            self.collapsed.insert(name.clone());
        }
        self.rebuild_rows();
        self.select_header(&name);
        true
    }

    /// Same, for whatever is selected. Returns false if that's an item.
    pub fn toggle_selected_category(&mut self) -> bool {
        match self.state.selected() {
            Some(i) => self.toggle_category_at(i),
            None => false,
        }
    }

    /// Left arrow: collapse the selected header, or jump from an item to
    /// its category header.
    pub fn collapse_or_jump(&mut self) {
        match self.selected_row().cloned() {
            Some(Row::Header { name, .. }) => {
                if self.collapsed.insert(name.clone()) {
                    self.rebuild_rows();
                }
                self.select_header(&name);
            }
            Some(Row::Item(idx)) => {
                if let Some(cat) = self.items[idx].category.clone() {
                    self.select_header(&cat);
                }
            }
            None => {}
        }
    }

    /// Right arrow: expand the selected header.
    pub fn expand_selected(&mut self) {
        if let Some(Row::Header { name, .. }) = self.selected_row().cloned() {
            if self.collapsed.remove(&name) {
                self.rebuild_rows();
                self.select_header(&name);
            }
        }
    }

    pub fn apply_filter(&mut self) {
        let q = self.query.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if q.is_empty() {
                    return true;
                }
                let hay =
                    format!("{} {} {}", item.label, item.desc, item.cmd).to_lowercase();
                fuzzy_match(&q, &hay)
            })
            .map(|(i, _)| i)
            .collect();
        self.rebuild_rows();
    }

    /// Rebuilds the visible row list from `filtered` + `collapsed` and
    /// keeps the selection valid. While searching, rows are a flat item
    /// list (no headers, collapse ignored) so every match is visible.
    pub fn rebuild_rows(&mut self) {
        self.rows.clear();
        if self.query.is_empty() {
            // Categories in order of first appearance in the config.
            let mut categories: Vec<String> = Vec::new();
            for &i in &self.filtered {
                if let Some(cat) = &self.items[i].category {
                    if !categories.iter().any(|c| c == cat) {
                        categories.push(cat.clone());
                    }
                }
            }
            for cat in &categories {
                let members: Vec<usize> = self
                    .filtered
                    .iter()
                    .copied()
                    .filter(|&i| self.items[i].category.as_deref() == Some(cat))
                    .collect();
                self.rows.push(Row::Header {
                    name: cat.clone(),
                    count: members.len(),
                });
                if !self.collapsed.contains(cat) {
                    self.rows.extend(members.into_iter().map(Row::Item));
                }
            }
            // Uncategorized items go last, ungrouped.
            self.rows.extend(
                self.filtered
                    .iter()
                    .copied()
                    .filter(|&i| self.items[i].category.is_none())
                    .map(Row::Item),
            );
        } else {
            self.rows = self.filtered.iter().map(|&i| Row::Item(i)).collect();
        }

        if self.rows.is_empty() {
            self.state.select(None);
        } else if self.query.is_empty() {
            let sel = self.state.selected().unwrap_or(0).min(self.rows.len() - 1);
            self.state.select(Some(sel));
        } else {
            // Fresh search: jump to the first (best) match.
            self.state.select(Some(0));
        }
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.spinner = (self.spinner + 1) % 10;
        // Drain background GitHub fetches only while that screen is open.
        if self.mode == Mode::Github {
            self.github.poll();
        }
    }
}

/// True if every char of `needle` appears in `haystack` in order
/// (subsequence match), so "lzg" matches "lazygit".
fn fuzzy_match(needle: &str, haystack: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|n| chars.by_ref().any(|h| h == n))
}

/// Looks up the local machine's hostname, uppercased for banner display.
/// Falls back to a placeholder if the OS doesn't report one.
fn get_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "UNKNOWN-HOST".to_string())
        .to_uppercase()
}
