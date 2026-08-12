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

- 🎨 **Retro BBS aesthetic** - ASCII art banner, cyan-on-dark theme, smooth animations
- ⌨️ **Keyboard-driven** - Navigate with `↑/↓` or `j/k`, select with `Enter`, quit with `q`
- ⚡ **Instant launching** - Pick a command and it fires immediately
- 🔧 **TOML config** - Easy-to-edit `bbs.toml` for all your shortcuts
- 🔍 **Fuzzy search** - Press `/` and type; `lzg` finds Lazygit
- 🗂️ **Foldable categories** - Group items under collapsible headers
- 📊 **Launch stats** - Remembers what you run and when
- 🐙 **GitHub dashboard screen** - A built-in, customizable all-in-one view (notifications, PRs, issues, starred repos, gists, profile) reusing your `gh` login — no extra tokens to manage
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
```

With `rainbow`, the accent walks the hue wheel and two extra effects kick
in:

- **Border chase** — every bordered pane gets a travelling gradient, like
  an LED strip. One full turn of the colour wheel is spread around each
  outline, so neighbouring cells differ by only a degree or two, and the
  whole pattern slides clockwise (a lap roughly every 12 seconds). A soft
  brightness wave rides along with it so the light visibly moves even
  across stretches where the hues are nearly identical.
- **Banner gradient** — the block letters carry the wheel across
  themselves rather than all sharing one tint.

Set `banner_animation = false` to freeze both into a static gradient.

## GitHub Dashboard

A built-in screen that shows your GitHub activity in one place: unread
notifications, PRs and issues assigned to you, starred repos, gists, and
your profile. It reuses your existing GitHub CLI login, so set that up
once and you're done:

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
refresh_secs = 120     # auto-refresh while the screen is open
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
```

## Planned

- [ ] Sound effects (optional module)
- [ ] Categories / tabbed menus
- [ ] Search/filter
- [ ] PowerShell/CMD/Tabby auto-launch
- [ ] Plugin system
- [ ] Weather widget
- [ ] System stats panel
