use crate::config::{BbsConfig, BbsItem};
use ratatui::widgets::ListState;

#[derive(Debug)]
pub struct App {
    pub config: BbsConfig,
    pub items: Vec<BbsItem>,
    pub state: ListState,
    pub status_message: String,
    pub spinner: usize,
    pub banner: String,
}

impl App {
    pub fn new(config: BbsConfig) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        let style = config
            .bbs
            .banner_style
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        let banner = blockfont::render(&get_hostname(), style);
        Self {
            config,
            items: Vec::new(),
            state,
            status_message: "Navigate: ↑/↓ or j/k  |  Launch: number key or Enter  |  Quit: q"
                .to_string(),
            spinner: 0,
            banner,
        }
    }

    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn get_selected(&self) -> Option<&BbsItem> {
        self.state.selected().and_then(|i| self.items.get(i))
    }

    pub fn find_by_key(&self, key: &str) -> Option<&BbsItem> {
        self.items.iter().find(|item| item.key == key)
    }

    pub fn update_spinner(&mut self) {
        self.spinner = (self.spinner + 1) % 4;
    }
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
