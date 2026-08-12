# BBS Launcher

A retro-futuristic BBS-style terminal launcher built in Rust with `ratatui` and `crossterm`.

## Project Structure

This is a Cargo workspace with two crates:

- **`bbs-launcher/`** — the TUI app itself (config loading, menu, event loop).
- **`blockfont/`** — a standalone, dependency-free library crate that renders text as block-letter ASCII art (`Shadow` and `Lined` styles). It has no ties to this app and can be reused or published on its own — see `blockfont/src/lib.rs`.

```
bbs-launcher/src/
  main.rs    entry point + CLI flags
  config.rs  bbs.toml loading/parsing
  app.rs     App state (menu rows, search, theme)
  ui.rs      ratatui drawing (banner/ticker/menu/details/status/footer)
  run.rs     event loop + command launching
  github.rs  built-in GitHub dashboard screen
  stats.rs   persisted launch counts
```

## Features

- 🎨 **Retro BBS aesthetic** - ASCII art banner, configurable accent colour, smooth animations
- 🌈 **Border chase** - A light travels around every pane like an LED strip
- ⌨️ **Keyboard-driven** - Navigate with `↑/↓` or `j/k`, select with `Enter`, quit with `q`
- ⚡ **Instant launching** - Pick a command and it fires; you land back on the menu when it exits
- 🔧 **TOML config** - Easy-to-edit `bbs.toml` for all your shortcuts
- ♻️ **Live reload** - Save `bbs.toml` and the running menu updates itself; a broken edit is reported in the status bar instead of crashing anything
- 🔍 **Ranked fuzzy search** - Press `/` and type; `lzg` finds Lazygit, results are ordered best-match-first, and the matched letters are highlighted
- 🗂️ **Foldable categories** - Group items under collapsible headers
- 📺 **Scrolling MOTD** - A marquee of your own messages under the banner
- 📊 **Launch stats** - Remembers what you run and when
- 🐙 **GitHub dashboard screen** - A built-in, customizable all-in-one view (notifications, PRs, issues, your repos, starred repos, gists, profile) reusing your `gh` login — no extra tokens to manage. The Repos tab lists every repo you have access to with stars/forks/open-issue counts, sortable on the fly with `s`
- 📺 **Complex menu items** - Items can open built-in screens instead of just running a command
- 🖥️ **Cross-terminal ready** - Tested on Windows Terminal, with PowerShell/CMD/Tabby support

## Quick Start

### Build

```bash
cargo build --release
```

The binary will be at `target/release/bbs-launcher.exe`.

### Configure

Edit `bbs.toml` (place it next to the binary, in your working directory, or at `~/.config/bbs-launcher/bbs.toml`):

```toml
[bbs]
title = "MY BBS"

[[items]]
key = "1"
label = "Claude Code"
cmd = "claude"
desc = "AI coding assistant"
icon = "CC"
color = "cyan"

[[items]]
key = "2"
label = "Neovim"
cmd = "nvim"
desc = "Text editor"
icon = "NV"
color = "green"

[[items]]
key = "Q"
label = "Quit"
cmd = "exit"
desc = "Close launcher"
icon = "QQ"
color = "red"
```

The first config found wins, so a `bbs.toml` beside the binary overrides
one in `~/.config`. `--config FILE` skips the search entirely, and
`--list` prints which file was used.

### `[bbs]` options

Everything here is optional except `title`.

| Option | Default | Effect |
|--------|---------|--------|
| `title` | — | Shown in the banner's top border |
| `subtitle` | none | Shown centred in the banner's bottom border |
| `banner_style` | `"shadow"` | Block-letter font: `shadow` (solid fill) or `lined` (horizontal-line fill). `lines`, `hatch`, and `striped` are accepted aliases |
| `theme` | `"cyan"` | Accent colour, or `rainbow` — see [Theming](#theming) |
| `banner_animation` | `true` | Animate the banner shimmer and the border chase |
| `border_chase` | `true` | Travelling light around every pane border |
| `chase_lap_secs` | `12.0` | Seconds per lap of the chase; lower is faster (clamped to 0.5–600) |
| `motd` | none | Lines for the scrolling ticker — see [Message of the Day](#message-of-the-day) |

The banner itself is your machine's hostname, uppercased; it isn't
configurable.

### Run

```bash
bbs-launcher
```

## Windows Terminal Integration

To launch automatically when opening Windows Terminal, edit your `settings.json`:

1. Open Windows Terminal settings (`Ctrl+,`)
2. Click "Open JSON file"
3. Add a new profile or modify an existing one:

```json
{
  "profiles": {
    "list": [
      {
        "name": "BBS Launcher",
        "commandline": "C:\\path\\to\\bbs-launcher.exe",
        "startingDirectory": "C:\\Users\\Lawrence",
        "hidden": false,
        "useAcrylic": true,
        "acrylicOpacity": 0.85,
        "colorScheme": "One Half Dark"
      }
    ]
  },
  "schemes": [
    {
      "name": "One Half Dark",
      "background": "#282c34",
      "foreground": "#abb2bf",
      "black": "#282c34",
      "red": "#e06c75",
      "green": "#98c379",
      "yellow": "#e5c07b",
      "blue": "#61afef",
      "magenta": "#c678dd",
      "cyan": "#56b6c2",
      "white": "#abb2bf",
      "brightBlack": "#5c6370",
      "brightRed": "#e06c75",
      "brightGreen": "#98c379",
      "brightYellow": "#e5c07b",
      "brightBlue": "#61afef",
      "brightMagenta": "#c678dd",
      "brightCyan": "#56b6c2",
      "brightWhite": "#ffffff"
    }
  ]
}
```

4. Set it as your default profile:

```json
"defaultProfile": "{YOUR-BBS-LAUNCHER-GUID}",
```

To get the GUID, run:
```powershell
Get-ChildItem Env: | Where-Object { $_.Name -like "*BBS*" }
```
Or just set `defaultProfile` to `"BBS Launcher"` if using the name string.

## Keybindings

| Key | Action |
|-----|--------|
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `g` / `G` | First / last row |
| `PgUp` / `PgDn` | Jump 5 rows |
| `←` / `→` | Collapse / expand a category |
| `Enter` | Launch the selected item, or fold a category header |
| `1`-`9`, … | Launch by hotkey (works even inside a collapsed category) |
| `/` | Fuzzy search by label, description, or command |
| `?` | Toggle the help overlay |
| Mouse | Scroll to move, click to select, double-click to launch |
| `q` / `Esc` | Quit |
| `Ctrl+C` | Force quit |

Launching does not exit the launcher — when the command finishes you
land back on the menu with its exit status in the status bar.

## Command Line

```bash
bbs-launcher                      # normal launch
bbs-launcher --config other.toml  # use a specific config
bbs-launcher --list               # print the resolved config and items, then exit
```

## Adding Items

Edit `bbs.toml` and add new `[[items]]` blocks:

```toml
[[items]]
key = "9"
label = "My Tool"
cmd = "mytool --flag"
desc = "Does something cool"
icon = "MT"
color = "magenta"
```

Optional per-item keys:

| Key | Effect |
|-----|--------|
| `wsl = true` | Run under WSL (`wsl bash -c`) instead of `cmd /C` |
| `cwd = "C:\\path"` | Launch from a specific working directory |
| `pause = true` | Wait for Enter before returning, so short-lived output stays readable |
| `category = "Develop"` | Group the item under a foldable header |
| `screen = "github"` | Open a built-in screen instead of running `cmd` |

## Categories

Items sharing a `category` are grouped under a foldable header, ordered
by where each category first appears in the file. Items without one are
listed last, ungrouped. Fold with `←`, `Enter`, or a click on the header;
hotkeys still work while a category is collapsed.

## Message of the Day

Add a `motd` list to `[bbs]` for a marquee that scrolls under the banner.
Omit it (or leave it empty) and the row disappears entirely:

```toml
[bbs]
motd = [
  "Welcome back",
  "Press / to search, ? for help",
]
```

## Launch Stats

Every launch is recorded to `~/.config/bbs-launcher/stats.toml`. Usage
counts appear as a dim `12×` badge next to each item, and the details
pane shows when you last ran it. Delete the file to reset.

## Theming

Set `theme` in `bbs.toml` to any accent color (`cyan`, `magenta`, `green`,
`yellow`, `red`, `blue`, `white`, …) or to `rainbow` to cycle the accent
through the full hue wheel:

```toml
[bbs]
theme = "rainbow"           # or e.g. "cyan"
banner_animation = true     # slow shimmer; false = static accent
border_chase = true         # travelling light around every pane border
chase_lap_secs = 12.0       # seconds per lap; lower is faster
```

### Border chase

Every bordered pane gets a light travelling clockwise around its outline,
like an LED strip. What travels depends on the theme:

- **`rainbow`** — a full turn of the colour wheel is spread around each
  outline, so neighbouring cells differ by only a degree or two and the
  gradient reads as diffused rather than banded. A soft brightness wave
  rides along so the motion is visible even where the hues are nearly
  identical.
- **Any solid colour** — the hue stays put and a narrow dim band chases
  through it instead, so the border mostly sits at your theme colour with
  a shadow running around it.

`chase_lap_secs` is how long one lap takes, so smaller numbers are
faster. Set `border_chase = false` for plain borders, or
`banner_animation = false` to freeze the pattern into a static gradient.

The `rainbow` theme also spreads the wheel across the banner's block
letters, rather than every glyph sharing one shifting tint.

## GitHub Dashboard

A built-in screen that shows your GitHub activity in one place: unread
notifications, open PRs and issues across the repos you work on, starred
repos, gists, and your profile. It reuses your existing GitHub CLI login,
so set that up once and you're done:

```bash
winget install GitHub.cli
gh auth login
```

(A `GH_TOKEN`/`GITHUB_TOKEN` environment variable works too.)

Add it to `bbs.toml` as a screen item (no `cmd` needed):

```toml
[[items]]
key = "9"
label = "GitHub"
desc = "All-in-one GitHub dashboard"
icon = "GH"
color = "white"
screen = "github"
```

### Customizing the screen

The `[github]` table in `bbs.toml` tunes what it shows (all options are
optional — defaults shown):

```toml
[github]
# Sections shown, in order. Unknown names are ignored.
# Default: all six below.
sections = ["notifications", "pull_requests", "issues", "stars", "gists", "profile"]
per_page = 25          # max entries per section (1-100)
refresh_secs = 120     # auto-refresh while the screen is open (minimum 5)
# Repo affiliation for the Issues and Pull Requests sections: comma-
# separated subset of owner, collaborator (write access), and
# organization_member. Defaults to all three.
affiliation = "owner,collaborator,organization_member"
```

The **Issues** and **Pull Requests** tabs list open items across every
repo you have write access to (own repos + repos where you're a
collaborator + org repos), most recently updated first.

Opening a notification goes to the real web page for its subject. The
API hands back an *api.github.com* URL (or none at all), so the link is
rewritten rather than passed through: resource names that are plural on
the API but singular on the web are corrected (`/pulls/7` → `/pull/7`),
and releases fall back to the repo's releases list because the API
identifies them by numeric id while the web addresses them by tag.
Notifications with no subject URL — CheckSuite results, Dependabot
alerts, discussions — land on the matching repo tab (`/actions`,
`/security/dependabot`, `/discussions`). There is no public page for a
notification thread id, so that route is never generated.

### GitHub screen keys

| Key | Action |
|-----|--------|
| `←`/`→` or `h`/`l` | Switch section tab |
| `↑`/`↓` or `j`/`k` | Move selection |
| `o` | Open the selected item in your browser |
| `m` | Mark the selected notification as read |
| `r` | Refresh all sections now |
| `q` / `Esc` | Back to the main menu |

All fetching runs on background threads, so the UI never freezes while
it talks to the GitHub API.

## Development

```bash
cargo run
cargo build --release
cargo test
cargo clippy --all-targets
```

Two tests are `#[ignore]`d because they aren't plain assertions:

```bash
# Print the whole rendered screen — handy for eyeballing layout changes
cargo test snapshot -- --ignored --nocapture

# Hit the real GitHub API; needs `gh` installed and authenticated
cargo test live_fetch_all_sections -- --ignored --nocapture
```

## Planned

- [ ] PowerShell/CMD/Tabby auto-launch
- [ ] Plugin system
- [ ] Weather widget
- [ ] System stats panel

Done since this list was written: foldable categories, fuzzy search, the
GitHub dashboard, launch stats, the MOTD ticker, and the border chase.
Sound effects were dropped — the `cpal` dependency was pulling in the
whole Windows audio stack unused, so it was removed.
