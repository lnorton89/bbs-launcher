//! Built-in GitHub dashboard screen.
//!
//! Fetching rides on the official GitHub CLI (`gh api`), which means we
//! reuse whatever auth the user already has (`gh auth login` — or a
//! `GH_TOKEN`/`GITHUB_TOKEN` env var, which `gh` honours automatically).
//! No HTTP stack, no token handling of our own.
//!
//! All network work happens on background threads that post results back
//! through an mpsc channel; the main loop drains it on every tick, so the
//! UI never blocks.

use crate::app::App;
use crate::config::GithubConfig;
use super::Nav;
use anyhow::{bail, Context, Result};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, widgets::ListState};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

/// How long a single `gh api` call may be outstanding before the section
/// assumes it is never coming back and allows a retry.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Messages from the background `gh api` workers back to the UI.
enum Msg {
    Section {
        idx: usize,
        /// Screen-session generation this fetch belongs to; messages
        /// from a previous visit are discarded by `poll`.
        gen: u64,
        /// Authenticated login, only set when the profile section loaded.
        owner: Option<String>,
        result: Result<Vec<Entry>>,
    },
    MarkedRead { id: String, ok: bool },
}

/// One selectable row in a GitHub section, plus everything the details
/// pane needs to render.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub title: String,
    pub subtitle: String,
    pub id: String,
    pub url: Option<String>,
    /// (label, value) pairs shown in the details pane.
    pub detail: Vec<(String, String)>,
    /// Sort keys for client-side re-sorting. Only the Repos section
    /// fills this in; everywhere else keeps server order.
    pub sort: Option<RepoSortKeys>,
}

/// The stats a repo row can be sorted by, extracted at parse time so a
/// sort never has to re-read the display strings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RepoSortKeys {
    pub name: String,
    pub stars: u64,
    pub forks: u64,
    pub open_issues: u64,
    /// Last push as epoch seconds; 0 when the repo has no pushes.
    pub pushed: i64,
}

/// Active sort order for the Repos section. Numeric sorts run
/// descending (biggest first); name sorts ascending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepoSort {
    #[default]
    Pushed,
    Stars,
    Forks,
    OpenIssues,
    Name,
}

impl RepoSort {
    const CYCLE: [RepoSort; 5] = [
        RepoSort::Pushed,
        RepoSort::Stars,
        RepoSort::Forks,
        RepoSort::OpenIssues,
        RepoSort::Name,
    ];

    pub fn parse(s: &str) -> Option<RepoSort> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pushed" | "updated" | "recent" => Some(RepoSort::Pushed),
            "stars" | "stargazers" => Some(RepoSort::Stars),
            "forks" => Some(RepoSort::Forks),
            "issues" | "open_issues" => Some(RepoSort::OpenIssues),
            "name" | "alpha" => Some(RepoSort::Name),
            _ => None,
        }
    }

    fn next(self) -> RepoSort {
        let i = Self::CYCLE.iter().position(|s| *s == self).unwrap_or(0);
        Self::CYCLE[(i + 1) % Self::CYCLE.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            RepoSort::Pushed => "recent push",
            RepoSort::Stars => "stars",
            RepoSort::Forks => "forks",
            RepoSort::OpenIssues => "open issues",
            RepoSort::Name => "name",
        }
    }
}

/// Sorts repo entries by the given key. Stable, so ties keep their
/// previous relative order; entries without keys sink to the bottom.
fn sort_repo_entries(entries: &mut [Entry], sort: RepoSort) {
    let key = |e: &Entry| e.sort.clone().unwrap_or_default();
    entries.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        match sort {
            RepoSort::Pushed => kb.pushed.cmp(&ka.pushed),
            RepoSort::Stars => kb.stars.cmp(&ka.stars),
            RepoSort::Forks => kb.forks.cmp(&ka.forks),
            RepoSort::OpenIssues => kb.open_issues.cmp(&ka.open_issues),
            RepoSort::Name => ka.name.to_lowercase().cmp(&kb.name.to_lowercase()),
        }
    });
}

/// The customizable sections of the dashboard, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Notifications,
    PullRequests,
    Issues,
    Repos,
    Stars,
    Gists,
    Profile,
}

impl SectionKind {
    pub const ALL: [SectionKind; 7] = [
        SectionKind::Notifications,
        SectionKind::PullRequests,
        SectionKind::Issues,
        SectionKind::Repos,
        SectionKind::Stars,
        SectionKind::Gists,
        SectionKind::Profile,
    ];

    pub fn parse(s: &str) -> Option<SectionKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "notifications" | "notif" | "inbox" => Some(SectionKind::Notifications),
            "pull_requests" | "pullrequests" | "prs" | "pr" => Some(SectionKind::PullRequests),
            "issues" | "issue" => Some(SectionKind::Issues),
            // "repos" used to alias the Stars section before this one
            // existed; it now means the user's own repositories.
            "repos" | "repositories" | "repositories_mine" => Some(SectionKind::Repos),
            "stars" | "starred" => Some(SectionKind::Stars),
            "gists" => Some(SectionKind::Gists),
            "profile" | "account" | "me" => Some(SectionKind::Profile),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SectionKind::Notifications => "Notifications",
            SectionKind::PullRequests => "Pull Requests",
            SectionKind::Issues => "Issues",
            SectionKind::Repos => "Repos",
            SectionKind::Stars => "Starred Repos",
            SectionKind::Gists => "Gists",
            SectionKind::Profile => "Profile",
        }
    }

    /// `gh api` arguments (path + query) for this section. The Issues and
    /// Pull Requests sections share the `/user/issues` endpoint (open
    /// items across every repo the user has access to), restricted to the
    /// configured affiliation; they differ only in how the response is
    /// split (PRs carry a `pull_request` key).
    fn gh_args(self, per_page: usize, affiliation: &str) -> Vec<String> {
        let pp = per_page.clamp(1, 100);
        match self {
            SectionKind::Notifications => vec![format!("/notifications?per_page={pp}")],
            SectionKind::PullRequests | SectionKind::Issues => vec![format!(
                "/user/issues?filter=all&state=open&affiliation={affiliation}\
                 &sort=updated&direction=desc&per_page=100"
            )],
            SectionKind::Repos => vec![format!(
                "/user/repos?affiliation={affiliation}&sort=pushed&direction=desc\
                 &per_page={pp}"
            )],
            SectionKind::Stars => vec![format!("/user/starred?sort=updated&per_page={pp}")],
            SectionKind::Gists => vec![format!("/gists?per_page={pp}")],
            SectionKind::Profile => vec!["/user".to_string()],
        }
    }

    /// Runs `gh api` (blocking) and parses the section data.
    /// Returns (authenticated login when this is the profile section,
    /// entries).
    fn fetch(self, per_page: usize, affiliation: &str) -> Result<(Option<String>, Vec<Entry>)> {
        let raw = gh_api(&self.gh_args(per_page, affiliation))?;
        let v: serde_json::Value =
            serde_json::from_slice(&raw).context("gh returned invalid JSON")?;
        match self {
            SectionKind::Notifications => parse_notifications(&v).map(|e| (None, e)),
            // The /user/issues fetch pulls a wide window (100) and the
            // split happens client-side, so a tab can't come back empty
            // just because the other type dominates the sort order.
            SectionKind::PullRequests => parse_user_issues(&v, true, per_page).map(|e| (None, e)),
            SectionKind::Issues => parse_user_issues(&v, false, per_page).map(|e| (None, e)),
            SectionKind::Repos => parse_repos(&v).map(|e| (None, e)),
            SectionKind::Stars => parse_stars(&v).map(|e| (None, e)),
            SectionKind::Gists => parse_gists(&v).map(|e| (None, e)),
            SectionKind::Profile => parse_profile(&v),
        }
    }
}

/// All the mutable state behind the GitHub screen.
#[derive(Debug)]
pub struct GithubView {
    pub sections: Vec<SectionKind>,
    pub per_page: usize,
    pub refresh_secs: u64,
    /// Repo affiliation filter for the issues/PR sections
    /// (comma-separated: owner, collaborator, organization_member).
    pub affiliation: String,
    pub tab: usize,
    /// Fetched rows per section (parallel to `sections`).
    pub entries: Vec<Vec<Entry>>,
    /// Per-tab list selection state.
    pub states: Vec<ListState>,
    /// Per-tab in-flight fetches.
    pub loading: Vec<bool>,
    /// When each in-flight fetch started, so one that never reports back
    /// can be retried instead of pinning the tab on "loading" forever.
    started: Vec<Option<Instant>>,
    /// Per-tab last error, if any.
    pub errors: Vec<Option<String>>,
    /// Authenticated login (once the profile section loads).
    pub owner: Option<String>,
    /// One-line connection status shown in the header.
    pub status: String,
    /// Where the list pane was last drawn, for mouse hit-testing.
    pub list_area: Option<Rect>,
    /// Active sort order for the Repos section.
    pub repo_sort: RepoSort,
    /// Bumped each time the screen opens, so stale in-flight fetches
    /// from a previous visit can't overwrite fresh data.
    generation: u64,
    last_refresh: Instant,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl GithubView {
    pub fn new(config: Option<GithubConfig>) -> Self {
        let cfg = config.unwrap_or_default();
        // Parse config section names, dropping aliases/duplicates while
        // keeping display order.
        let mut parsed: Vec<SectionKind> = Vec::new();
        if let Some(list) = cfg.sections {
            for name in list {
                if let Some(kind) = SectionKind::parse(&name) {
                    if !parsed.contains(&kind) {
                        parsed.push(kind);
                    }
                }
            }
        }
        let sections = if parsed.is_empty() {
            SectionKind::ALL.to_vec()
        } else {
            parsed
        };
        let per_page = cfg.per_page.unwrap_or(25).clamp(1, 100);
        let refresh_secs = cfg.refresh_secs.unwrap_or(120).max(5);
        // Empty/whitespace-only affiliation falls back to the default.
        let affiliation = cfg
            .affiliation
            .map(|a| a.replace(' ', ""))
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "owner,collaborator,organization_member".into());
        let repo_sort = cfg
            .repo_sort
            .as_deref()
            .and_then(RepoSort::parse)
            .unwrap_or_default();
        let count = sections.len();
        let (tx, rx) = channel();
        GithubView {
            sections,
            per_page,
            refresh_secs,
            affiliation,
            tab: 0,
            entries: vec![Vec::new(); count],
            states: (0..count).map(|_| ListState::default()).collect(),
            loading: vec![false; count],
            started: vec![None; count],
            errors: vec![None; count],
            owner: None,
            status: "waiting for connection…".into(),
            list_area: None,
            repo_sort,
            generation: 0,
            last_refresh: Instant::now(),
            tx,
            rx,
        }
    }

    /// Called when the screen is opened. Fresh fetch of everything.
    pub fn open(&mut self) {
        self.status = "connecting…".into();
        // Invalidate any in-flight fetches from a previous visit.
        self.generation = self.generation.wrapping_add(1);
        // Those fetches will be discarded on arrival for having a stale
        // generation, so they must not also count as in-flight — leaving
        // the flags set would make `spawn_fetch` below skip every section
        // and strand the screen on "loading" for the rest of the session.
        self.loading.iter_mut().for_each(|l| *l = false);
        self.refresh_all();
    }

    pub fn refresh_all(&mut self) {
        self.last_refresh = Instant::now();
        for i in 0..self.sections.len() {
            self.spawn_fetch(i);
        }
    }

    fn spawn_fetch(&mut self, idx: usize) {
        if idx >= self.sections.len() {
            return;
        }
        // Skip sections already fetching — but treat one that has been
        // in flight far longer than any `gh` call should take as lost, so
        // a wedged request gets retried rather than blocking the section
        // permanently.
        let in_flight = self.loading[idx]
            && self.started[idx].is_some_and(|t| t.elapsed() < FETCH_TIMEOUT);
        if in_flight {
            return;
        }
        self.loading[idx] = true;
        self.started[idx] = Some(Instant::now());
        let tx = self.tx.clone();
        let kind = self.sections[idx];
        let per_page = self.per_page;
        let affiliation = self.affiliation.clone();
        let gen = self.generation;
        std::thread::spawn(move || {
            // catch_unwind so a panic in a parser can't leave the tab
            // stuck on "loading…" forever.
            let result =
                std::panic::catch_unwind(|| kind.fetch(per_page, &affiliation));
            let msg = match result {
                Ok(Ok((owner, entries))) => Msg::Section {
                    idx,
                    gen,
                    owner,
                    result: Ok(entries),
                },
                Ok(Err(e)) => Msg::Section {
                    idx,
                    gen,
                    owner: None,
                    result: Err(e),
                },
                Err(_) => Msg::Section {
                    idx,
                    gen,
                    owner: None,
                    result: Err(anyhow::anyhow!("fetch worker panicked")),
                },
            };
            let _ = tx.send(msg);
        });
    }

    /// Drains worker messages and auto-refreshes on the timer. Call on
    /// every tick while the screen is open.
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Section {
                    idx,
                    gen,
                    owner,
                    result,
                } => {
                    if gen != self.generation {
                        // Stale result from a previous screen visit.
                        continue;
                    }
                    if idx < self.loading.len() {
                        self.loading[idx] = false;
                    }
                    match result {
                        Ok(mut entries) => {
                            if idx < self.entries.len() {
                                // The repos list honours whatever sort
                                // the user has dialled in, even across
                                // refreshes.
                                if self.sections.get(idx) == Some(&SectionKind::Repos) {
                                    sort_repo_entries(&mut entries, self.repo_sort);
                                }
                                self.entries[idx] = entries;
                                self.errors[idx] = None;
                                if self.states[idx].selected().is_none()
                                    && !self.entries[idx].is_empty()
                                {
                                    self.states[idx].select(Some(0));
                                }
                            }
                            if let Some(o) = owner {
                                self.owner = Some(o.clone());
                                self.status = format!("connected as @{o}");
                            } else if self.status.starts_with("connecting") {
                                self.status = "connected".into();
                            }
                        }
                        Err(e) => {
                            let text = format!("{e:#}");
                            if idx < self.errors.len() {
                                self.errors[idx] = Some(text.clone());
                            }
                            let is_profile = self
                                .sections
                                .get(idx)
                                .copied()
                                == Some(SectionKind::Profile);
                            let authy = text.contains("Bad credentials")
                                || text.contains("401")
                                || text.contains("403")
                                || text.contains("not authenticated");
                            if is_profile || (self.owner.is_none() && authy) {
                                self.status = if authy {
                                    "not authenticated — run `gh auth login`".into()
                                } else {
                                    "connection failed — check gh install & auth".into()
                                };
                            }
                        }
                    }
                }
                Msg::MarkedRead { id, ok } => {
                    if ok {
                        if let Some(i) = self
                            .sections
                            .iter()
                            .position(|s| *s == SectionKind::Notifications)
                        {
                            self.entries[i].retain(|e| e.id != id);
                        }
                        self.status = "notification marked as read".into();
                    } else {
                        self.status = "mark-as-read failed".into();
                    }
                }
            }
        }
        if self.last_refresh.elapsed() >= Duration::from_secs(self.refresh_secs) {
            self.refresh_all();
        }
    }

    pub fn next(&mut self) {
        self.move_selection(1);
    }

    pub fn previous(&mut self) {
        self.move_selection(-1);
    }

    pub fn jump(&mut self, delta: i64) {
        self.move_selection(delta);
    }

    fn move_selection(&mut self, delta: i64) {
        let n = self.entries.get(self.tab).map(|e| e.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        let cur = self.states[self.tab].selected().unwrap_or(0) as i64;
        let next = (cur + delta).clamp(0, n as i64 - 1) as usize;
        self.states[self.tab].select(Some(next));
    }

    pub fn select_first(&mut self) {
        if let Some(st) = self.states.get_mut(self.tab) {
            st.select(Some(0));
        }
    }

    pub fn select_last(&mut self) {
        let n = self.entries.get(self.tab).map(|e| e.len()).unwrap_or(0);
        if n > 0 {
            self.states[self.tab].select(Some(n - 1));
        }
    }

    pub fn next_tab(&mut self) {
        if !self.sections.is_empty() {
            self.tab = (self.tab + 1) % self.sections.len();
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.sections.is_empty() {
            self.tab = (self.tab + self.sections.len() - 1) % self.sections.len();
        }
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        let idx = self.states.get(self.tab)?.selected()?;
        self.entries.get(self.tab)?.get(idx)
    }

    pub fn open_selected(&mut self) {
        let Some(url) = self.selected_entry().and_then(|e| e.url.clone()) else {
            return;
        };
        // Hand the URL to the shell's default handler directly. Avoid
        // `cmd /C start`: cmd mangles URLs — pre-quoted args arrive with
        // stray backslashes, and `&` inside an unquoted URL is treated as
        // a command separator. rundll32 has no shell parsing to fight.
        let url = url.replace('"', "");
        match std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", &url])
            .spawn()
        {
            Ok(_) => self.status = "opened in browser".into(),
            Err(_) => self.status = "couldn't open browser".into(),
        }
    }

    pub fn mark_selected_read(&mut self) {
        if self.sections.get(self.tab).copied() != Some(SectionKind::Notifications) {
            self.status = "mark-as-read only applies to notifications".into();
            return;
        }
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if entry.id.is_empty() {
            return;
        }
        let id = entry.id;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let ok = mark_notification_read(&id).is_ok();
            let _ = tx.send(Msg::MarkedRead { id, ok });
        });
        self.status = "marking as read…".into();
    }

    /// Steps the Repos tab to the next sort order and re-sorts in place,
    /// keeping the cursor on the same repo so cycling never loses the
    /// user's place. A no-op (with a hint) on any other tab.
    pub fn cycle_repo_sort(&mut self) {
        if self.sections.get(self.tab) != Some(&SectionKind::Repos) {
            self.status = "sorting applies to the Repos tab".into();
            return;
        }
        self.repo_sort = self.repo_sort.next();
        let followed = self.selected_entry().map(|e| e.id.clone());
        sort_repo_entries(&mut self.entries[self.tab], self.repo_sort);
        if let Some(id) = followed {
            if let Some(pos) = self.entries[self.tab].iter().position(|e| e.id == id) {
                self.states[self.tab].select(Some(pos));
            }
        }
        self.status = format!("repos sorted by {}", self.repo_sort.label());
    }
}

pub fn handle_key(app: &mut App, key: KeyEvent) -> Nav {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Nav::Back,
        KeyCode::Char('h') | KeyCode::Left => {
            app.github.prev_tab();
            Nav::Stay
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.github.next_tab();
            Nav::Stay
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.github.next();
            Nav::Stay
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.github.previous();
            Nav::Stay
        }
        KeyCode::PageDown => {
            app.github.jump(5);
            Nav::Stay
        }
        KeyCode::PageUp => {
            app.github.jump(-5);
            Nav::Stay
        }
        KeyCode::Home | KeyCode::Char('g') => {
            app.github.select_first();
            Nav::Stay
        }
        KeyCode::End | KeyCode::Char('G') => {
            app.github.select_last();
            Nav::Stay
        }
        KeyCode::Char('r') => {
            app.github.refresh_all();
            Nav::Stay
        }
        KeyCode::Char('o') => {
            app.github.open_selected();
            Nav::Stay
        }
        KeyCode::Char('m') => {
            app.github.mark_selected_read();
            Nav::Stay
        }
        KeyCode::Char('s') => {
            app.github.cycle_repo_sort();
            Nav::Stay
        }
        _ => Nav::Stay,
    }
}

// ─────────────────────────── gh api transport ───────────────────────────

/// Runs `gh api <args>` and returns the raw stdout bytes.
fn gh_api(args: &[String]) -> Result<Vec<u8>> {
    let output = std::process::Command::new("gh")
        .arg("api")
        .args(args)
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "GitHub CLI (gh) not available: {e}. Install it with `winget install GitHub.cli` \
                 or from https://cli.github.com, then run `gh auth login`."
            )
        })?;
    if !output.status.success() {
        bail!(
            "gh api failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn mark_notification_read(id: &str) -> Result<()> {
    let out = std::process::Command::new("gh")
        .arg("api")
        .arg("-X")
        .arg("PATCH")
        .arg(format!("/notifications/threads/{id}"))
        .output()
        .context("failed to run gh")?;
    if !out.status.success() {
        bail!(
            "gh api failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

// ─────────────────────────────── parsers ────────────────────────────────

fn parse_notifications(v: &serde_json::Value) -> Result<Vec<Entry>> {
    #[derive(serde::Deserialize)]
    struct Subject {
        title: String,
        r#type: String,
        url: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Repo {
        full_name: String,
    }
    #[derive(serde::Deserialize)]
    struct Item {
        id: String,
        unread: bool,
        reason: String,
        subject: Subject,
        repository: Repo,
        updated_at: String,
    }
    let items: Vec<Item> = serde_json::from_value(v.clone()).context("bad notifications payload")?;
    Ok(items
        .into_iter()
        .map(|n| {
            let url = notification_url(
                &n.subject.r#type,
                n.subject.url.as_deref(),
                &n.repository.full_name,
            );
            let reason = reason_label(&n.reason);
            Entry {
                title: n.subject.title,
                subtitle: format!("{} · {}", n.repository.full_name, reason),
                id: n.id,
                url: Some(url),
                detail: vec![
                    ("Repository".into(), n.repository.full_name),
                    ("Type".into(), n.subject.r#type),
                    ("Reason".into(), reason),
                    (
                        "State".into(),
                        if n.unread { "unread".into() } else { "read".into() },
                    ),
                    ("Updated".into(), pretty_date(&n.updated_at)),
                ],
                sort: None,
            }
        })
        .collect())
}

/// Maps a notification onto a web page that actually resolves.
///
/// The notifications API gives an *API* url for the subject (or none at
/// all), and swapping the host alone isn't enough: several REST resource
/// names are pluralized where the matching web route is singular
/// (`/pulls/7` → `/pull/7`, `/commits/SHA` → `/commit/SHA`), and releases
/// are addressed by numeric id on the API but by tag on the web. When
/// there's no subject url, we send the user to the repo tab that
/// notification came from — there is no public web page for a
/// notification thread id (`/notifications/beta/threads/<id>` 404s).
fn notification_url(kind: &str, subject_url: Option<&str>, repo: &str) -> String {
    let repo_page = format!("https://github.com/{}", repo.trim_matches('/'));

    if let Some(api) = subject_url {
        if let Some(rest) = api.strip_prefix("https://api.github.com/repos/") {
            let rest = rest.split(['?', '#']).next().unwrap_or(rest);
            let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
            if segs.len() >= 4 {
                let (owner, name, resource, id) = (segs[0], segs[1], segs[2], segs[3]);
                let base = format!("https://github.com/{owner}/{name}");
                match resource {
                    "pulls" => return format!("{base}/pull/{id}"),
                    "commits" => return format!("{base}/commit/{id}"),
                    "issues" | "discussions" => {
                        return format!("{base}/{resource}/{id}")
                    }
                    // Only the numeric id is available here, but the web
                    // route wants the tag — the list page is the closest
                    // link that resolves.
                    "releases" => return format!("{base}/releases"),
                    _ => {}
                }
            }
        }
    }

    match kind {
        "CheckSuite" => format!("{repo_page}/actions"),
        "Discussion" => format!("{repo_page}/discussions"),
        "Release" => format!("{repo_page}/releases"),
        "RepositoryVulnerabilityAlert" => format!("{repo_page}/security/dependabot"),
        "RepositoryInvitation" => format!("{repo_page}/invitations"),
        _ => repo_page,
    }
}

/// Parses a `/user/issues` response (a bare JSON array of issues *and*
/// pull requests). When `want_prs` is true only items carrying the
/// `pull_request` key are kept, otherwise only plain issues. `limit`
/// caps the result AFTER the split, so a tab shows `limit` items of its
/// own kind even when the other kind dominates the fetch window.
fn parse_user_issues(v: &serde_json::Value, want_prs: bool, limit: usize) -> Result<Vec<Entry>> {
    #[derive(serde::Deserialize)]
    struct Item {
        number: usize,
        title: String,
        state: String,
        html_url: String,
        created_at: String,
        updated_at: String,
        comments: usize,
        repository_url: String,
        /// Null for issues by deleted users.
        user: Option<User>,
        /// Present (non-null) exactly when this item is a pull request.
        pull_request: Option<serde_json::Value>,
    }
    #[derive(serde::Deserialize)]
    struct User {
        login: String,
    }
    let items: Vec<Item> = serde_json::from_value(v.clone()).context("bad /user/issues payload")?;
    Ok(items
        .into_iter()
        .filter(|i| i.pull_request.is_some() == want_prs)
        .take(limit.max(1))
        .map(|i| {
            let repo = i
                .repository_url
                .trim_start_matches("https://api.github.com/repos/")
                .to_string();
            let author = i
                .user
                .map(|u| u.login)
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| "ghost".to_string());
            Entry {
                title: format!("#{} {}", i.number, i.title),
                subtitle: format!("{} · @{}", repo, author),
                id: format!("#{}", i.number),
                url: Some(i.html_url),
                detail: vec![
                    ("Repository".into(), repo),
                    ("State".into(), i.state),
                    ("Author".into(), author),
                    ("Comments".into(), i.comments.to_string()),
                    ("Created".into(), pretty_date(&i.created_at)),
                    ("Updated".into(), pretty_date(&i.updated_at)),
                ],
                sort: None,
            }
        })
        .collect())
}

/// Parses a `/user/repos` response: every repo the user owns or has
/// access to via the affiliation filter, with the stats the Repos tab
/// shows and sorts by.
fn parse_repos(v: &serde_json::Value) -> Result<Vec<Entry>> {
    #[derive(serde::Deserialize)]
    struct Item {
        full_name: String,
        description: Option<String>,
        html_url: String,
        private: bool,
        fork: bool,
        archived: bool,
        language: Option<String>,
        stargazers_count: u64,
        forks_count: u64,
        open_issues_count: u64,
        /// Null for repos that have never been pushed to.
        pushed_at: Option<String>,
    }
    let items: Vec<Item> = serde_json::from_value(v.clone()).context("bad repos payload")?;
    Ok(items
        .into_iter()
        .map(|r| {
            let pushed_secs = r
                .pushed_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.timestamp())
                .unwrap_or(0);
            let language = r.language.clone().unwrap_or_else(|| "—".into());
            // Compact stat line for the list row: stars, forks, open
            // issues, language, then any flags that change how you'd
            // treat the repo.
            let mut subtitle = format!(
                "★{} ⑂{} !{} · {}",
                r.stargazers_count, r.forks_count, r.open_issues_count, language
            );
            for (on, flag) in [
                (r.private, "private"),
                (r.fork, "fork"),
                (r.archived, "archived"),
            ] {
                if on {
                    subtitle.push_str(" · ");
                    subtitle.push_str(flag);
                }
            }
            let mut detail = vec![
                ("Stars".into(), r.stargazers_count.to_string()),
                ("Forks".into(), r.forks_count.to_string()),
                ("Open issues".into(), r.open_issues_count.to_string()),
                ("Language".into(), language),
                (
                    "Visibility".into(),
                    if r.private { "private".into() } else { "public".into() },
                ),
                (
                    "Pushed".into(),
                    r.pushed_at
                        .as_deref()
                        .map(pretty_date)
                        .unwrap_or_else(|| "never".into()),
                ),
            ];
            if r.fork {
                detail.push(("Fork".into(), "yes".into()));
            }
            if r.archived {
                detail.push(("Archived".into(), "yes".into()));
            }
            if let Some(desc) = r.description.filter(|d| !d.is_empty()) {
                detail.push(("Description".into(), desc));
            }
            Entry {
                title: r.full_name.clone(),
                subtitle,
                id: r.full_name.clone(),
                url: Some(r.html_url),
                detail,
                sort: Some(RepoSortKeys {
                    name: r.full_name,
                    stars: r.stargazers_count,
                    forks: r.forks_count,
                    open_issues: r.open_issues_count,
                    pushed: pushed_secs,
                }),
            }
        })
        .collect())
}

fn parse_stars(v: &serde_json::Value) -> Result<Vec<Entry>> {
    #[derive(serde::Deserialize)]
    struct Item {
        full_name: String,
        description: Option<String>,
        html_url: String,
        language: Option<String>,
        stargazers_count: usize,
        forks_count: usize,
        updated_at: String,
        archived: bool,
    }
    let items: Vec<Item> = serde_json::from_value(v.clone()).context("bad stars payload")?;
    Ok(items
        .into_iter()
        .map(|r| {
            let mut detail = vec![
                ("Stars".into(), r.stargazers_count.to_string()),
                ("Forks".into(), r.forks_count.to_string()),
                ("Language".into(), r.language.clone().unwrap_or_else(|| "—".into())),
                (
                    "Archived".into(),
                    if r.archived { "yes".into() } else { "no".into() },
                ),
                ("Updated".into(), pretty_date(&r.updated_at)),
            ];
            if let Some(desc) = r.description.clone() {
                if !desc.is_empty() {
                    detail.push(("Description".into(), desc));
                }
            }
            Entry {
                title: r.full_name.clone(),
                subtitle: r
                    .description
                    .unwrap_or_else(|| "no description".into()),
                id: r.full_name,
                url: Some(r.html_url),
                detail,
                sort: None,
            }
        })
        .collect())
}

fn parse_gists(v: &serde_json::Value) -> Result<Vec<Entry>> {
    #[derive(serde::Deserialize)]
    struct Item {
        id: String,
        description: Option<String>,
        html_url: String,
        public: bool,
        created_at: String,
        updated_at: String,
        files: serde_json::Map<String, serde_json::Value>,
    }
    let items: Vec<Item> = serde_json::from_value(v.clone()).context("bad gists payload")?;
    Ok(items
        .into_iter()
        .map(|g| {
            let names = g
                .files
                .keys()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let title = g
                .description
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| names.clone());
            Entry {
                title,
                subtitle: format!(
                    "{} · {} file(s)",
                    if g.public { "public" } else { "secret" },
                    g.files.len()
                ),
                id: g.id,
                url: Some(g.html_url),
                detail: vec![
                    ("Files".into(), names),
                    (
                        "Visibility".into(),
                        if g.public { "public".into() } else { "secret".into() },
                    ),
                    ("Created".into(), pretty_date(&g.created_at)),
                    ("Updated".into(), pretty_date(&g.updated_at)),
                ],
                sort: None,
            }
        })
        .collect())
}

fn parse_profile(v: &serde_json::Value) -> Result<(Option<String>, Vec<Entry>)> {
    #[derive(serde::Deserialize)]
    struct Profile {
        login: String,
        name: Option<String>,
        bio: Option<String>,
        html_url: String,
        followers: usize,
        following: usize,
        public_repos: usize,
        location: Option<String>,
        company: Option<String>,
        blog: Option<String>,
        created_at: String,
    }
    let p: Profile = serde_json::from_value(v.clone()).context("bad profile payload")?;
    let display = p.name.clone().unwrap_or_else(|| p.login.clone());
    let entry = Entry {
        title: format!("@{} — {}", p.login, display),
        subtitle: p.bio.clone().unwrap_or_default(),
        id: p.login.clone(),
        url: Some(p.html_url),
        detail: vec![
            ("Name".into(), p.name.unwrap_or_else(|| "—".into())),
            ("Followers".into(), p.followers.to_string()),
            ("Following".into(), p.following.to_string()),
            ("Public repos".into(), p.public_repos.to_string()),
            ("Location".into(), p.location.unwrap_or_else(|| "—".into())),
            ("Company".into(), p.company.unwrap_or_else(|| "—".into())),
            ("Blog".into(), p.blog.unwrap_or_else(|| "—".into())),
            ("Joined".into(), pretty_date(&p.created_at)),
        ],
        sort: None,
    };
    Ok((Some(p.login), vec![entry]))
}

fn reason_label(reason: &str) -> String {
    match reason {
        "assign" => "assigned".to_string(),
        "author" => "authored".to_string(),
        "comment" => "commented".to_string(),
        "mention" => "mentioned".to_string(),
        "review_requested" => "review requested".to_string(),
        "team_mention" => "team mentioned".to_string(),
        "subscribed" => "subscribed".to_string(),
        "ci_activity" => "CI activity".to_string(),
        other => other.to_string(),
    }
}

fn pretty_date(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(dt) => {
            let secs = dt.timestamp().max(0) as u64;
            // Relative for recent timestamps, an actual date otherwise
            // (e.g. a profile "Joined" from years ago).
            if crate::stats::now_secs().saturating_sub(secs) < 90 * 86_400 {
                crate::stats::time_ago(secs)
            } else {
                dt.format("%b %Y").to_string()
            }
        }
        Err(_) => iso.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn view_config_fallbacks_and_dedupes() {
        // No config -> all sections, defaults.
        let v = GithubView::new(None);
        assert_eq!(v.sections.len(), SectionKind::ALL.len());
        assert_eq!(v.per_page, 25);
        assert_eq!(v.refresh_secs, 120);

        // Aliases and duplicates collapse to one tab; out-of-range
        // values get clamped.
        let cfg = GithubConfig {
            sections: Some(vec![
                "profile".into(),
                "me".into(),
                "bogus".into(),
                "profile".into(),
            ]),
            per_page: Some(500),
            refresh_secs: Some(2),
            affiliation: Some(" owner , collaborator ".into()),
            repo_sort: Some("stars".into()),
        };
        let v = GithubView::new(Some(cfg));
        assert_eq!(v.sections, vec![SectionKind::Profile]);
        assert_eq!(v.per_page, 100);
        assert_eq!(v.refresh_secs, 5);
        assert_eq!(v.affiliation, "owner,collaborator");
        assert_eq!(v.repo_sort, RepoSort::Stars);

        // All-invalid names fall back to the full set.
        let cfg = GithubConfig {
            sections: Some(vec!["wat".into()]),
            repo_sort: Some("wat".into()),
            ..Default::default()
        };
        let v = GithubView::new(Some(cfg));
        assert_eq!(v.sections.len(), SectionKind::ALL.len());
        assert_eq!(v.affiliation, "owner,collaborator,organization_member");
        assert_eq!(v.repo_sort, RepoSort::Pushed, "bad sort name falls back");

        // Blank affiliation falls back to the default too.
        let cfg = GithubConfig {
            affiliation: Some("   ".into()),
            ..Default::default()
        };
        let v = GithubView::new(Some(cfg));
        assert_eq!(v.affiliation, "owner,collaborator,organization_member");
    }

    #[test]
    fn reopening_never_strands_a_section_on_loading() {
        let mut v = GithubView::new(None);

        // Visit one: fetches go out and are still in flight when the user
        // leaves (nothing drains the channel outside the screen).
        v.open();
        assert!(v.loading.iter().all(|l| *l), "all sections start fetching");

        // Those workers report back late, tagged with the old generation.
        let stale = v.generation;
        for idx in 0..v.sections.len() {
            v.tx.send(Msg::Section {
                idx,
                gen: stale,
                owner: None,
                result: Ok(Vec::new()),
            })
            .unwrap();
        }

        // Visit two must start fresh fetches rather than seeing the
        // abandoned ones as in-flight and skipping every section.
        v.open();
        assert!(
            v.loading.iter().all(|l| *l),
            "reopening must re-arm every section"
        );
        assert_ne!(v.generation, stale);

        // Draining the stale replies must not clear the new fetches'
        // flags, and must not populate the new generation with their data.
        v.poll();
        assert!(
            v.loading.iter().all(|l| *l),
            "stale replies must not cancel the current fetches"
        );
    }

    #[test]
    fn a_wedged_fetch_is_retried_rather_than_blocking_forever() {
        let mut v = GithubView::new(None);
        v.open();
        assert!(v.loading[0]);

        // Nothing ever replies. Within the timeout the section holds.
        v.spawn_fetch(0);
        assert!(v.started[0].is_some_and(|t| t.elapsed() < FETCH_TIMEOUT));

        // Once the fetch is older than the timeout it is considered lost
        // and the next refresh re-arms it instead of skipping it.
        v.started[0] = Some(Instant::now() - FETCH_TIMEOUT - Duration::from_secs(1));
        v.spawn_fetch(0);
        assert!(
            v.started[0].is_some_and(|t| t.elapsed() < FETCH_TIMEOUT),
            "a timed-out fetch should have been restarted"
        );
    }

    #[test]
    fn section_names_parse_with_aliases() {
        assert_eq!(SectionKind::parse("notifications"), Some(SectionKind::Notifications));
        assert_eq!(SectionKind::parse("PRs"), Some(SectionKind::PullRequests));
        assert_eq!(SectionKind::parse("issue"), Some(SectionKind::Issues));
        // "repos" means the user's own repositories (it used to alias
        // Stars before the Repos section existed).
        assert_eq!(SectionKind::parse("repos"), Some(SectionKind::Repos));
        assert_eq!(SectionKind::parse("repositories"), Some(SectionKind::Repos));
        assert_eq!(SectionKind::parse("starred"), Some(SectionKind::Stars));
        assert_eq!(SectionKind::parse("gists"), Some(SectionKind::Gists));
        assert_eq!(SectionKind::parse("me"), Some(SectionKind::Profile));
        assert_eq!(SectionKind::parse("wat"), None);
    }

    #[test]
    fn gh_args_use_the_right_queries() {
        let aff = "owner,collaborator";
        let args = SectionKind::PullRequests.gh_args(25, aff);
        assert!(args[0].starts_with("/user/issues?filter=all&state=open"));
        assert!(args[0].contains("affiliation=owner,collaborator"));
        // Fetches a wide window so the client-side PR/issue split can't
        // starve either tab.
        assert!(args[0].contains("per_page=100"));
        let args = SectionKind::Issues.gh_args(25, aff);
        assert!(args[0].starts_with("/user/issues?filter=all"));
        assert!(args[0].contains("sort=updated&direction=desc"));
        let args = SectionKind::Repos.gh_args(25, aff);
        assert!(args[0].starts_with("/user/repos?affiliation=owner,collaborator"));
        assert!(args[0].contains("sort=pushed&direction=desc"));
        assert!(args[0].contains("per_page=25"));
        let args = SectionKind::Notifications.gh_args(25, aff);
        assert!(args[0].starts_with("/notifications?per_page=25"));
        let args = SectionKind::Profile.gh_args(25, aff);
        assert_eq!(args[0], "/user");
    }

    /// End-to-end check against the real GitHub API. Requires `gh`
    /// installed and authenticated; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs gh CLI + network"]
    fn live_fetch_all_sections() {
        let aff = "owner,collaborator,organization_member";
        let mut failures = Vec::new();
        for kind in SectionKind::ALL {
            eprintln!("fetching {}", kind.label());
            match kind.fetch(5, aff) {
                Ok((owner, entries)) => println!(
                    "{} -> {} entries{}",
                    kind.label(),
                    entries.len(),
                    owner.map(|o| format!(" (owner {o})")).unwrap_or_default()
                ),
                Err(e) => failures.push(format!("{}: {e:#}", kind.label())),
            }
        }
        // Notifications: every entry must link to a github.com web page,
        // never an api.github.com path or the dead beta-threads route.
        let raw = gh_api(&SectionKind::Notifications.gh_args(25, aff)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        for e in parse_notifications(&v).unwrap() {
            let url = e.url.clone().unwrap_or_default();
            assert!(
                url.starts_with("https://github.com/"),
                "not a web url: {url}"
            );
            assert!(!url.contains("notifications/beta"), "dead route: {url}");
            assert!(!url.contains("/pulls/"), "API-plural pull url: {url}");
        }
        assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
    }

    #[test]
    fn notifications_parse_and_rewrite_urls() {
        // Subject with an API url -> direct web link.
        let v = json(r#"[{
            "id": "42",
            "unread": true,
            "reason": "review_requested",
            "subject": {"title": "Add dashboard", "type": "PullRequest", "url": "https://api.github.com/repos/octo/app/pulls/7"},
            "repository": {"full_name": "octo/app"},
            "updated_at": "2026-08-01T10:00:00Z"
        }, {
            "id": "99",
            "unread": true,
            "reason": "ci_activity",
            "subject": {"title": "CI failed", "type": "CheckSuite", "url": null},
            "repository": {"full_name": "octo/app"},
            "updated_at": "2026-08-02T10:00:00Z"
        }]"#);
        let entries = parse_notifications(&v).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].subtitle, "octo/app · review requested");
        // The API path is /pulls/7; the web route is singular.
        assert_eq!(
            entries[0].url.as_deref(),
            Some("https://github.com/octo/app/pull/7")
        );
        // No subject url -> the repo tab this notification came from.
        assert_eq!(
            entries[1].url.as_deref(),
            Some("https://github.com/octo/app/actions")
        );
        assert_eq!(entries[1].id, "99");
    }

    #[test]
    fn notification_urls_resolve_to_real_web_pages() {
        let u = |kind: &str, url: Option<&str>| notification_url(kind, url, "octo/app");

        // API resource names that are singular on the web.
        assert_eq!(
            u("PullRequest", Some("https://api.github.com/repos/octo/app/pulls/7")),
            "https://github.com/octo/app/pull/7"
        );
        assert_eq!(
            u("Commit", Some("https://api.github.com/repos/octo/app/commits/abc123")),
            "https://github.com/octo/app/commit/abc123"
        );
        // Already-matching names pass through.
        assert_eq!(
            u("Issue", Some("https://api.github.com/repos/octo/app/issues/12")),
            "https://github.com/octo/app/issues/12"
        );
        // Releases are keyed by tag on the web, so the id can't be used.
        assert_eq!(
            u("Release", Some("https://api.github.com/repos/octo/app/releases/98765")),
            "https://github.com/octo/app/releases"
        );
        // Query strings don't leak into the path.
        assert_eq!(
            u("Issue", Some("https://api.github.com/repos/octo/app/issues/12?foo=1")),
            "https://github.com/octo/app/issues/12"
        );

        // Subject-less notifications land on the relevant repo tab.
        assert_eq!(u("CheckSuite", None), "https://github.com/octo/app/actions");
        assert_eq!(u("Discussion", None), "https://github.com/octo/app/discussions");
        assert_eq!(
            u("RepositoryVulnerabilityAlert", None),
            "https://github.com/octo/app/security/dependabot"
        );
        assert_eq!(u("Mystery", None), "https://github.com/octo/app");

        // Nothing ever produces the dead beta-threads route.
        for kind in ["CheckSuite", "Discussion", "Release", "Mystery"] {
            assert!(!u(kind, None).contains("notifications/beta"));
        }
    }

    #[test]
    fn user_issues_parse_splits_prs_and_issues() {
        let v = json(r#"[{
            "number": 12, "title": "Fix the bug", "state": "open",
            "html_url": "https://github.com/octo/app/issues/12",
            "created_at": "2026-07-01T10:00:00Z", "updated_at": "2026-07-02T10:00:00Z",
            "comments": 3, "repository_url": "https://api.github.com/repos/octo/app",
            "user": {"login": "octocat"}
        }, {
            "number": 7, "title": "Add dashboard", "state": "open",
            "html_url": "https://github.com/octo/app/pull/7",
            "created_at": "2026-07-03T10:00:00Z", "updated_at": "2026-07-04T10:00:00Z",
            "comments": 1, "repository_url": "https://api.github.com/repos/octo/app",
            "user": {"login": "octocat"},
            "pull_request": {"url": "https://api.github.com/repos/octo/app/pulls/7"}
        }]"#);
        let issues = parse_user_issues(&v, false, 25).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].title, "#12 Fix the bug");
        assert_eq!(issues[0].url.as_deref(), Some("https://github.com/octo/app/issues/12"));
        let prs = parse_user_issues(&v, true, 25).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].title, "#7 Add dashboard");
        assert_eq!(prs[0].url.as_deref(), Some("https://github.com/octo/app/pull/7"));
        assert!(prs[0].detail.iter().any(|(k, val)| k == "Repository" && val == "octo/app"));
        // The limit applies after the split: a tiny limit caps each kind
        // independently.
        let limited = parse_user_issues(&v, true, 1).unwrap();
        assert_eq!(limited.len(), 1);
        let none = parse_user_issues(&v, false, 1).unwrap();
        assert_eq!(none.len(), 1);
    }

    #[test]
    fn user_issues_tolerate_deleted_users() {
        let v = json(r#"[{
            "number": 5, "title": "Old issue", "state": "open",
            "html_url": "https://github.com/octo/app/issues/5",
            "created_at": "2026-01-01T10:00:00Z", "updated_at": "2026-01-02T10:00:00Z",
            "comments": 0, "repository_url": "https://api.github.com/repos/octo/app",
            "user": null
        }]"#);
        let issues = parse_user_issues(&v, false, 25).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].subtitle, "octo/app · @ghost");
        assert!(issues[0].detail.iter().any(|(k, val)| k == "Author" && val == "ghost"));
    }

    #[test]
    fn profile_parse_returns_owner() {
        let v = json(r#"{
            "login": "lnorton89", "name": "Lawrence", "bio": "dev",
            "html_url": "https://github.com/lnorton89",
            "followers": 10, "following": 20, "public_repos": 5,
            "location": "Earth", "company": null, "blog": "", "created_at": "2020-01-01T00:00:00Z"
        }"#);
        let (owner, entries) = parse_profile(&v).unwrap();
        assert_eq!(owner.as_deref(), Some("lnorton89"));
        assert_eq!(entries[0].title, "@lnorton89 — Lawrence");
        assert!(entries[0].detail.iter().any(|(k, _)| k == "Followers"));
    }

    /// Three repos with deliberately conflicting stat orderings, so
    /// every sort key produces a different winner.
    fn repo_fixture() -> serde_json::Value {
        json(r#"[{
            "full_name": "octo/zebra", "description": "stripes", "html_url": "https://github.com/octo/zebra",
            "private": false, "fork": false, "archived": false, "language": "Rust",
            "stargazers_count": 5, "forks_count": 9, "open_issues_count": 1,
            "pushed_at": "2026-08-01T00:00:00Z"
        }, {
            "full_name": "octo/alpha", "description": null, "html_url": "https://github.com/octo/alpha",
            "private": true, "fork": true, "archived": false, "language": null,
            "stargazers_count": 50, "forks_count": 2, "open_issues_count": 7,
            "pushed_at": "2026-06-01T00:00:00Z"
        }, {
            "full_name": "octo/mid", "description": "", "html_url": "https://github.com/octo/mid",
            "private": false, "fork": false, "archived": true, "language": "Go",
            "stargazers_count": 20, "forks_count": 4, "open_issues_count": 3,
            "pushed_at": null
        }]"#)
    }

    #[test]
    fn repos_parse_with_stats_and_sort_keys() {
        let entries = parse_repos(&repo_fixture()).unwrap();
        assert_eq!(entries.len(), 3);

        // The list row carries the stats and any state flags.
        assert_eq!(entries[0].subtitle, "★5 ⑂9 !1 · Rust");
        assert_eq!(entries[1].subtitle, "★50 ⑂2 !7 · — · private · fork");
        assert_eq!(entries[2].subtitle, "★20 ⑂4 !3 · Go · archived");

        // Sort keys are extracted, with a never-pushed repo keyed to 0.
        let keys = entries[1].sort.as_ref().unwrap();
        assert_eq!((keys.stars, keys.forks, keys.open_issues), (50, 2, 7));
        assert!(keys.pushed > 0);
        assert_eq!(entries[2].sort.as_ref().unwrap().pushed, 0);

        // The details pane gets the full stat table.
        let detail = &entries[0].detail;
        for (k, want) in [("Stars", "5"), ("Forks", "9"), ("Open issues", "1")] {
            assert!(
                detail.iter().any(|(dk, dv)| dk == k && dv == want),
                "missing {k}={want} in {detail:?}"
            );
        }
        assert!(entries[2]
            .detail
            .iter()
            .any(|(k, v)| k == "Pushed" && v == "never"));
    }

    #[test]
    fn repo_sorts_order_by_each_key() {
        let names = |entries: &[Entry]| {
            entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>()
        };
        let mut entries = parse_repos(&repo_fixture()).unwrap();

        sort_repo_entries(&mut entries, RepoSort::Stars);
        assert_eq!(names(&entries), ["octo/alpha", "octo/mid", "octo/zebra"]);
        sort_repo_entries(&mut entries, RepoSort::Forks);
        assert_eq!(names(&entries), ["octo/zebra", "octo/mid", "octo/alpha"]);
        sort_repo_entries(&mut entries, RepoSort::OpenIssues);
        assert_eq!(names(&entries), ["octo/alpha", "octo/mid", "octo/zebra"]);
        sort_repo_entries(&mut entries, RepoSort::Name);
        assert_eq!(names(&entries), ["octo/alpha", "octo/mid", "octo/zebra"]);
        // Recency: the never-pushed repo sinks to the bottom.
        sort_repo_entries(&mut entries, RepoSort::Pushed);
        assert_eq!(names(&entries), ["octo/zebra", "octo/alpha", "octo/mid"]);
    }

    #[test]
    fn cycling_sort_reorders_and_follows_the_selection() {
        let cfg = GithubConfig {
            sections: Some(vec!["repos".into()]),
            ..Default::default()
        };
        let mut v = GithubView::new(Some(cfg));
        assert_eq!(v.sections, vec![SectionKind::Repos]);
        assert_eq!(v.repo_sort, RepoSort::Pushed);
        v.entries[0] = parse_repos(&repo_fixture()).unwrap();
        // Cursor on octo/mid (fetched order: zebra, alpha, mid).
        v.states[0].select(Some(2));

        v.cycle_repo_sort();
        assert_eq!(v.repo_sort, RepoSort::Stars);
        assert_eq!(v.entries[0][0].id, "octo/alpha", "stars order applied");
        let followed = v.states[0].selected().unwrap();
        assert_eq!(
            v.entries[0][followed].id, "octo/mid",
            "cursor follows the same repo through the re-sort"
        );
        assert!(v.status.contains("stars"));

        // The full cycle wraps back around to the start.
        for _ in 0..4 {
            v.cycle_repo_sort();
        }
        assert_eq!(v.repo_sort, RepoSort::Pushed);
    }

    #[test]
    fn sort_key_is_a_noop_hint_on_other_tabs() {
        let cfg = GithubConfig {
            sections: Some(vec!["gists".into()]),
            ..Default::default()
        };
        let mut v = GithubView::new(Some(cfg));
        let before = v.repo_sort;
        v.cycle_repo_sort();
        assert_eq!(v.repo_sort, before, "sort state untouched off the Repos tab");
        assert!(v.status.contains("Repos tab"));
    }

    #[test]
    fn fetched_repos_arrive_pre_sorted_by_the_active_order() {
        let cfg = GithubConfig {
            sections: Some(vec!["repos".into()]),
            repo_sort: Some("stars".into()),
            ..Default::default()
        };
        let mut v = GithubView::new(Some(cfg));
        v.open();
        // A worker delivers repos in fetch order; poll must store them
        // in the user's chosen order.
        v.tx.send(Msg::Section {
            idx: 0,
            gen: v.generation,
            owner: None,
            result: Ok(parse_repos(&repo_fixture()).unwrap()),
        })
        .unwrap();
        v.poll();
        assert_eq!(v.entries[0][0].id, "octo/alpha");
        assert_eq!(v.entries[0][1].id, "octo/mid");
        assert_eq!(v.entries[0][2].id, "octo/zebra");
    }

    #[test]
    fn gists_and_stars_parse() {
        let g = json(r#"[{
            "id": "g1", "description": "dotfiles", "html_url": "https://gist.github.com/g1",
            "public": true, "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-02-01T00:00:00Z",
            "files": {"bashrc": {}, "vimrc": {}}
        }]"#);
        let entries = parse_gists(&g).unwrap();
        assert_eq!(entries[0].title, "dotfiles");
        assert_eq!(entries[0].subtitle, "public · 2 file(s)");

        let s = json(r#"[{
            "full_name": "octo/app", "description": "an app", "html_url": "https://github.com/octo/app",
            "language": "Rust", "stargazers_count": 42, "forks_count": 7,
            "updated_at": "2026-06-01T00:00:00Z", "archived": false
        }]"#);
        let entries = parse_stars(&s).unwrap();
        assert_eq!(entries[0].title, "octo/app");
        assert!(entries[0].detail.iter().any(|(k, val)| k == "Stars" && val == "42"));
    }
}
