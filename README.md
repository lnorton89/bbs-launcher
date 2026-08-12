# BBS Launcher

A retro-futuristic BBS-style terminal launcher built in Rust with `ratatui` and `crossterm`.

## Project Structure

This is a Cargo workspace with two crates:

- **`bbs-launcher/`** — the TUI app itself (config loading, menu, event loop).
- **`blockfont/`** — a standalone, dependency-free library crate that renders text as block-letter ASCII art (`Shadow` and `Lined` styles). It has no ties to this app and can be reused or published on its own — see `blockfont/src/lib.rs`.

```
bbs-launcher/src/
  main.rs    entry point
  config.rs  bbs.toml loading/parsing
  app.rs     App state
  ui.rs      ratatui drawing (banner/menu/status/footer)
  run.rs     event loop + command launching
```

## Features

- 🎨 **Retro BBS aesthetic** - ASCII art banner, cyan-on-dark theme, smooth animations
- ⌨️ **Keyboard-driven** - Navigate with `↑/↓` or `j/k`, select with `Enter`, quit with `q`
- ⚡ **Instant launching** - Pick a command and it fires immediately
- 🔧 **TOML config** - Easy-to-edit `bbs.toml` for all your shortcuts
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
| `Enter` | Launch selected item |
| `q` | Quit |
| `Esc` | Quit |
| `Ctrl+C` | Force quit |

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
