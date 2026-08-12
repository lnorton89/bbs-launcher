use crate::screens::bluetti::BluettiView;
use crate::config::{BbsConfig, BbsItem};
use crate::screens::github::GithubView;
use crate::stats::Stats;
use crate::ui::{color_from_str, hsv_to_rgb};
use ratatui::{layout::Rect, style::Color, widgets::ListState};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// UI ticks per second. Animation phases are derived from the tick
/// counter, so this is what converts configured durations into the
/// per-tick steps the drawing code works in.
///
/// Kept deliberately low: every tick recolours the banner, the border
/// chase, and the ticker — around a thousand cells — and sustained
/// escape-code throughput is what pushes ConPTY into visual corruption.
/// Input latency is unaffected; key and mouse events interrupt the
/// tick wait immediately.
pub const TICKS_PER_SEC: f32 = 5.0;

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
    /// Built-in Bluetti power-station monitor screen.
    Bluetti,
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

/// How menu items are ordered within their category. Cycled with `s`;
/// the starting order comes from the config (`menu_sort`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MenuSort {
    /// The order items appear in bbs.toml.
    #[default]
    Config,
    /// Highest launch count first.
    Launches,
    /// Most recently launched first.
    Recent,
}

impl MenuSort {
    pub fn parse(s: &str) -> Option<MenuSort> {
        match s.trim().to_ascii_lowercase().as_str() {
            "config" | "manual" => Some(MenuSort::Config),
            "launches" | "count" | "most_used" => Some(MenuSort::Launches),
            "recent" | "recency" | "last_used" => Some(MenuSort::Recent),
            _ => None,
        }
    }

    fn next(self) -> MenuSort {
        match self {
            MenuSort::Config => MenuSort::Launches,
            MenuSort::Launches => MenuSort::Recent,
            MenuSort::Recent => MenuSort::Config,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MenuSort::Config => "config order",
            MenuSort::Launches => "most launched",
            MenuSort::Recent => "recently used",
        }
    }
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
    /// Indices into `items` that match the current search query, best
    /// match first while a query is active.
    pub filtered: Vec<usize>,
    /// For each matched item, the char positions in its search haystack
    /// (`"label desc cmd"`) that the query hit, so the menu can
    /// highlight them. Empty when not searching.
    pub match_positions: HashMap<usize, Vec<usize>>,
    /// What the menu actually shows: headers + visible items, derived
    /// from `filtered` and `collapsed`. `state` selects within this.
    pub rows: Vec<Row>,
    pub collapsed: HashSet<String>,
    /// Active in-category ordering of menu items.
    pub menu_sort: MenuSort,
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
    /// Same path as `config_path`, kept as a real `PathBuf` for the
    /// file watcher and reloads.
    pub config_file: PathBuf,
    /// Last seen mtime of the config file; a change triggers a live
    /// reload. `None` when the file is missing or unreadable.
    config_mtime: Option<SystemTime>,
    /// Pre-joined message-of-the-day text for the scrolling ticker.
    /// `None` when no `motd` is configured, which hides the row entirely.
    pub motd: Option<String>,
    /// State for the built-in GitHub dashboard screen.
    pub github: GithubView,
    /// State for the built-in Bluetti monitor screen.
    pub bluetti: BluettiView,
}

impl App {
    pub fn new(config: BbsConfig, config_path: PathBuf) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        let github = GithubView::new(config.github.clone());
        let bluetti = BluettiView::new(config.bluetti.clone());
        let config_mtime = file_mtime(&config_path);
        let items = config.items.clone();
        let mut app = Self {
            config,
            items,
            state,
            status_message: "Ready".to_string(),
            spinner: 0,
            banner: String::new(),
            mode: Mode::Normal,
            query: String::new(),
            filtered: Vec::new(),
            match_positions: HashMap::new(),
            rows: Vec::new(),
            collapsed: HashSet::new(),
            menu_sort: MenuSort::default(),
            stats: Stats::load(),
            tick: 0,
            theme: Theme::Solid(Color::Cyan),
            animate: true,
            chase: true,
            chase_degrees_per_tick: 0.0,
            menu_area: None,
            last_click: None,
            session_start: Instant::now(),
            config_path: config_path.display().to_string(),
            config_file: config_path,
            config_mtime,
            motd: None,
            github,
            bluetti,
        };
        app.refresh_derived();
        app.apply_filter();
        app
    }

    /// Recomputes everything that is a pure function of `self.config`:
    /// banner art, theme, animation switches, chase speed, and the motd
    /// ticker text. Called on startup and again on every live reload.
    fn refresh_derived(&mut self) {
        let style = self
            .config
            .bbs
            .banner_style
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        self.banner = blockfont::render(&get_hostname(), style);
        self.theme = self
            .config
            .bbs
            .theme
            .as_deref()
            .map(Theme::parse)
            .unwrap_or(Theme::Solid(Color::Cyan));
        self.animate = self.config.bbs.banner_animation.unwrap_or(true);
        // Reject NaN/infinity before clamping, so a nonsense value falls
        // back to the default rather than freezing the chase.
        let chase_lap_secs = self
            .config
            .bbs
            .chase_lap_secs
            .filter(|s| s.is_finite())
            .map(|s| s.clamp(MIN_CHASE_LAP_SECS, MAX_CHASE_LAP_SECS))
            .unwrap_or(DEFAULT_CHASE_LAP_SECS);
        self.chase_degrees_per_tick = 360.0 / (chase_lap_secs * TICKS_PER_SEC);
        self.chase = self.config.bbs.border_chase.unwrap_or(true);
        self.menu_sort = self
            .config
            .bbs
            .menu_sort
            .as_deref()
            .and_then(MenuSort::parse)
            .unwrap_or_default();
        // Blank/whitespace-only entries are dropped so a stray empty
        // string in the config can't produce a row of dead space.
        self.motd = self.config.bbs.motd.as_ref().and_then(|lines| {
            let joined = lines
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("   ✦   ");
            (!joined.is_empty()).then(|| format!("{joined}   ✦   "))
        });
    }

    /// Re-reads the config file and applies it in place, preserving
    /// session state: selection, folded categories, search query, stats,
    /// and uptime. A file that fails to read or parse leaves the running
    /// config untouched and reports the problem in the status bar, so a
    /// half-saved edit can never take down a live session.
    pub fn reload_config(&mut self) {
        let config = match crate::config::load_config(Some(self.config_file.clone())) {
            Ok((config, _)) => config,
            Err(err) => {
                self.status_message = format!("Config reload failed: {err:#}");
                return;
            }
        };
        let selected = self.selected_item().map(|i| i.label.clone());
        // Replacing the GitHub view drops its cached entries, so keep it
        // unless its slice of the config actually changed.
        if self.config.github != config.github {
            self.github = GithubView::new(config.github.clone());
        }
        // Same for the Bluetti subscriber: redial only when its slice
        // changed, and stop the old thread so it doesn't linger.
        if self.config.bluetti != config.bluetti {
            self.bluetti.stop();
            self.bluetti = BluettiView::new(config.bluetti.clone());
        }
        self.items = config.items.clone();
        self.config = config;
        self.refresh_derived();
        // Categories that no longer exist can't stay folded.
        let items = &self.items;
        self.collapsed
            .retain(|c| items.iter().any(|i| i.category.as_deref() == Some(c.as_str())));
        self.apply_filter();
        if let Some(label) = selected {
            self.select_label(&label);
        }
        self.status_message = "Config reloaded".to_string();
    }

    /// The accent color this frame. Solid themes are constant; the
    /// rainbow theme walks the hue wheel as the app ticks.
    pub fn accent(&self) -> Color {
        match self.theme {
            Theme::Solid(c) => c,
            Theme::Rainbow => {
                // 4.8°/tick at 5 ticks/sec = a lap every 15 seconds.
                let hue = if self.animate {
                    (self.tick as f32 * 4.8) % 360.0
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

    /// Records a click on row `idx` and reports whether it completed a
    /// double-click (second click on an already-selected row within the
    /// double-click window).
    pub fn register_click(&mut self, idx: usize, was_selected: bool) -> bool {
        let now = Instant::now();
        let is_double = was_selected
            && self.last_click.take().is_some_and(|(t, i)| {
                i == idx
                    && now.duration_since(t) < std::time::Duration::from_millis(450)
            });
        self.last_click = Some((now, idx));
        is_double
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
        self.match_positions.clear();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let mut scored: Vec<(i32, usize)> = Vec::new();
            for (i, item) in self.items.iter().enumerate() {
                let hay = format!("{} {} {}", item.label, item.desc, item.cmd);
                if let Some((score, positions)) = fuzzy_score(&q, &hay) {
                    scored.push((score, i));
                    self.match_positions.insert(i, positions);
                }
            }
            // Best match first; ties keep config order (sort is stable).
            scored.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
            self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        }
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
                    self.rows
                        .extend(self.order_by_menu_sort(members).into_iter().map(Row::Item));
                }
            }
            // Uncategorized items go last, ungrouped.
            let loose: Vec<usize> = self
                .filtered
                .iter()
                .copied()
                .filter(|&i| self.items[i].category.is_none())
                .collect();
            self.rows
                .extend(self.order_by_menu_sort(loose).into_iter().map(Row::Item));
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

    /// Reorders item indices for display according to the active menu
    /// sort. Stable, so equal keys keep config order; items that have
    /// never been launched sink below launched ones under both stats
    /// sorts.
    fn order_by_menu_sort(&self, mut indices: Vec<usize>) -> Vec<usize> {
        match self.menu_sort {
            MenuSort::Config => {}
            MenuSort::Launches => indices.sort_by_key(|&i| {
                std::cmp::Reverse(
                    self.stats.get(&self.items[i].label).map(|s| s.count).unwrap_or(0),
                )
            }),
            MenuSort::Recent => indices.sort_by_key(|&i| {
                std::cmp::Reverse(
                    self.stats
                        .get(&self.items[i].label)
                        .and_then(|s| s.last_launched)
                        .unwrap_or(0),
                )
            }),
        }
        indices
    }

    /// Steps to the next menu sort, keeping the cursor on the same item.
    pub fn cycle_menu_sort(&mut self) {
        let selected = self.selected_item().map(|i| i.label.clone());
        self.menu_sort = self.menu_sort.next();
        self.rebuild_rows();
        if let Some(label) = selected {
            self.select_label(&label);
        }
        self.status_message = format!("menu sorted by {}", self.menu_sort.label());
    }

    pub fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.spinner = (self.spinner + 1) % 10;
        // Drain background GitHub fetches only while that screen is open.
        if self.mode == Mode::Github {
            self.github.poll();
        }
        // The Bluetti subscriber streams continuously once started, so
        // its channel is drained every tick — otherwise it would buffer
        // unboundedly while the screen is closed.
        self.bluetti.poll();
        let watching = matches!(self.mode, Mode::Normal | Mode::Search | Mode::Help);
        if watching && self.tick.is_multiple_of(TICKS_PER_SEC as u64) {
            // Watch the config file for edits about once a second.
            // Skipped while a built-in screen is open so a reload can't
            // tear down its live state mid-view; a pending change is
            // picked up as soon as the menu is back.
            let mtime = file_mtime(&self.config_file);
            if mtime != self.config_mtime {
                self.config_mtime = mtime;
                // A vanished file (mid-save, or deleted) isn't a reload;
                // keep running on the loaded config until it reappears.
                if mtime.is_some() {
                    self.reload_config();
                }
            }
        }
    }
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Scores how well `needle` matches `haystack` as a case-insensitive
/// subsequence ("lzg" matches "lazygit"), returning the score and the
/// haystack char positions that matched — or `None` when it doesn't
/// match at all. Word-start hits and unbroken runs score higher, and a
/// match that starts earlier beats one buried deep in the string, so
/// "git" ranks "Git Status" above "Lazygit". Matching is greedy
/// left-to-right, which is cheap and close enough for menu-sized lists.
fn fuzzy_score(needle: &str, haystack: &str) -> Option<(i32, Vec<usize>)> {
    let mut positions = Vec::new();
    let mut score = 0i32;
    let mut wanted = needle.chars().map(lower_char).peekable();
    let mut prev: Option<char> = None;
    let mut prev_matched = false;
    for (i, h) in haystack.chars().enumerate() {
        let Some(&n) = wanted.peek() else { break };
        if lower_char(h) == n {
            wanted.next();
            score += 1;
            if prev_matched {
                score += 5;
            }
            if prev.is_none_or(|p| !p.is_alphanumeric()) {
                score += 8;
            }
            positions.push(i);
            prev_matched = true;
        } else {
            prev_matched = false;
        }
        prev = Some(h);
    }
    if wanted.peek().is_some() {
        return None;
    }
    if let Some(&first) = positions.first() {
        score -= (first as i32).min(15);
    }
    Some((score, positions))
}

/// First char of the lowercase mapping — a per-char fold that keeps
/// haystack indices stable, which full-string `to_lowercase` (which can
/// change the char count) would not.
fn lower_char(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_score_is_a_subsequence_match_with_positions() {
        assert!(fuzzy_score("xyz", "lazygit").is_none());
        let (_, pos) = fuzzy_score("lzg", "lazygit").unwrap();
        assert_eq!(pos, vec![0, 2, 4], "l, z, g of 'lazygit'");
        // Case-insensitive both ways.
        assert!(fuzzy_score("git", "GIT STATUS").is_some());
        // The empty query matches everything with no highlights.
        assert_eq!(fuzzy_score("", "anything"), Some((0, vec![])));
    }

    #[test]
    fn fuzzy_ranking_prefers_word_starts_runs_and_early_matches() {
        let score = |n: &str, h: &str| fuzzy_score(n, h).unwrap().0;
        // Word-start match beats the same letters mid-word.
        assert!(score("git", "Git Status") > score("git", "Lazygit"));
        // An unbroken run beats scattered letters.
        assert!(score("re", "read") > score("re", "grep everything"));
        // Matching early in the string beats matching deep into it.
        assert!(score("mon", "monitor") > score("mon", "system monitor"));
    }

    fn write_config(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("bbs-launcher-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}-{}.toml", name, std::process::id()));
        std::fs::write(&path, body).unwrap();
        path
    }

    const BASE: &str = r#"
[bbs]
title = "T"

[[items]]
key = "1"
label = "Alpha"
cmd = "a"
desc = "first"
icon = "A"
color = "cyan"
category = "Tools"

[[items]]
key = "2"
label = "Beta"
cmd = "b"
desc = "second"
icon = "B"
color = "red"
category = "Tools"
"#;

    fn app_from(path: &Path) -> App {
        let (config, path) = crate::config::load_config(Some(path.to_path_buf())).unwrap();
        App::new(config, path)
    }

    #[test]
    fn reload_applies_edits_and_keeps_the_selection() {
        let path = write_config("reload", BASE);
        let mut app = app_from(&path);
        assert_eq!(app.items.len(), 2);
        app.select_label("Beta");

        // Retitle, and grow the menu by one item.
        let grown = format!(
            "{BASE}\n[[items]]\nkey = \"3\"\nlabel = \"Gamma\"\ncmd = \"g\"\n\
             desc = \"third\"\nicon = \"G\"\ncolor = \"green\"\n"
        )
        .replace("title = \"T\"", "title = \"T2\"");
        std::fs::write(&path, grown).unwrap();
        app.reload_config();

        assert_eq!(app.config.bbs.title, "T2");
        assert_eq!(app.items.len(), 3);
        assert_eq!(app.status_message, "Config reloaded");
        assert_eq!(
            app.selected_item().map(|i| i.label.clone()),
            Some("Beta".into()),
            "selection survives the reload"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn broken_edit_keeps_the_running_config() {
        let path = write_config("broken", BASE);
        let mut app = app_from(&path);
        std::fs::write(&path, "this is not toml [").unwrap();
        app.reload_config();
        assert_eq!(app.items.len(), 2, "old config stays in effect");
        assert!(
            app.status_message.starts_with("Config reload failed"),
            "the error is surfaced: {}",
            app.status_message
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reload_drops_folds_of_categories_that_no_longer_exist() {
        let path = write_config("folds", BASE);
        let mut app = app_from(&path);
        app.collapsed.insert("Tools".into());
        app.rebuild_rows();

        // Recategorize everything: "Tools" disappears.
        std::fs::write(&path, BASE.replace("category = \"Tools\"", "")).unwrap();
        app.reload_config();
        assert!(app.collapsed.is_empty(), "stale fold state is pruned");
        assert_eq!(app.items.len(), 2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn hotkeys_resolve_case_insensitively_to_the_right_item() {
        // Against the real workspace bbs.toml, so a config/binding drift
        // (like B landing on anything but the Bluetti screen) fails CI.
        let config_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("bbs.toml");
        let (config, path) = crate::config::load_config(Some(config_path)).unwrap();
        let app = App::new(config, path);

        let item = app.find_by_key("b").expect("lowercase b resolves");
        assert_eq!(item.label, "Bluetti");
        assert_eq!(item.screen.as_deref(), Some("bluetti"));
        let item = app.find_by_key("B").expect("uppercase B resolves");
        assert_eq!(item.label, "Bluetti");
        let item = app.find_by_key("8").unwrap();
        assert_eq!(item.label, "Python REPL");
        assert!(app.find_by_key("z").is_none());
    }

    #[test]
    fn menu_sort_cycles_orders_by_stats_and_keeps_the_selection() {
        let path = write_config("menu-sort", BASE);
        let mut app = app_from(&path);
        let labels = |app: &App| {
            app.rows
                .iter()
                .filter_map(|r| match r {
                    Row::Item(i) => Some(app.items[*i].label.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(app.menu_sort, MenuSort::Config);
        assert_eq!(labels(&app), ["Alpha", "Beta"]);

        // Beta is the only launched item, so "most launched" leads with
        // it — and the cursor stays on the item it was on.
        app.stats.record("Beta");
        app.select_label("Beta");
        app.cycle_menu_sort();
        assert_eq!(app.menu_sort, MenuSort::Launches);
        assert_eq!(labels(&app), ["Beta", "Alpha"]);
        assert_eq!(
            app.selected_item().map(|i| i.label.clone()),
            Some("Beta".into())
        );
        assert!(app.status_message.contains("most launched"));

        // Recent: Alpha's launch is newer, so it leads.
        app.stats.record("Alpha");
        app.stats.items.get_mut("Alpha").unwrap().last_launched =
            Some(crate::stats::now_secs() + 1_000);
        app.cycle_menu_sort();
        assert_eq!(app.menu_sort, MenuSort::Recent);
        assert_eq!(labels(&app), ["Alpha", "Beta"]);

        // And the cycle wraps back to config order.
        app.cycle_menu_sort();
        assert_eq!(app.menu_sort, MenuSort::Config);
        assert_eq!(labels(&app), ["Alpha", "Beta"]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn menu_sort_starting_order_comes_from_the_config() {
        let body = BASE.replace("title = \"T\"", "title = \"T\"\nmenu_sort = \"launches\"");
        let path = write_config("menu-sort-config", &body);
        let app = app_from(&path);
        assert_eq!(app.menu_sort, MenuSort::Launches);
        let _ = std::fs::remove_file(path);

        assert_eq!(MenuSort::parse("recent"), Some(MenuSort::Recent));
        assert_eq!(MenuSort::parse("wat"), None);
    }

    #[test]
    fn search_ranks_best_match_first() {
        let path = write_config("rank", BASE);
        let mut app = app_from(&path);
        // "s" hits both items, but at a word start for Beta ("second")
        // and mid-word for Alpha ("first") — so ranking must put Beta
        // first, reversing config order.
        app.query = "s".into();
        app.apply_filter();
        assert_eq!(app.filtered.len(), 2, "both items match");
        assert_eq!(
            app.selected_item().map(|i| i.label.clone()),
            Some("Beta".into())
        );
        // And the matched positions are recorded for highlighting:
        // in the "Beta second b" haystack the s sits at char 5.
        let idx = app.filtered[0];
        assert_eq!(app.match_positions.get(&idx), Some(&vec![5]));
        let _ = std::fs::remove_file(path);
    }
}
