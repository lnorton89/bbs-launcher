use super::effects::is_border_glyph;
use super::menu::marquee;
use super::draw;
use crate::app::{App, Mode, Row};
use crate::screens::github::Entry;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;


fn test_app() -> App {
    // Point at the workspace bbs.toml (tests run with cwd inside
    // target/, where find_config would miss it).
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("bbs.toml");
    let (config, path) = crate::config::load_config(Some(config_path)).unwrap();
    App::new(config, path)
}

fn buffer_text(app: &mut App) -> String {
    let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[test]
#[ignore = "visual check; run with --ignored --nocapture"]
fn snapshot() {
    let mut app = test_app();
    let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|f| draw(f, &mut app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    for y in 0..buf.area.height {
        let row: String = (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol())
            .collect();
        println!("{row}");
    }
}

#[test]
fn main_menu_renders() {
    let text = buffer_text(&mut test_app());
    assert!(text.contains("Main Menu"));
    assert!(text.contains("GitHub"));
    assert!(text.contains("Details"));
}

#[test]
fn category_headers_render_and_fold() {
    let mut app = test_app();
    // The sample config groups items under headers.
    let text = buffer_text(&mut app);
    assert!(text.contains("DEVELOP"), "category header should render");
    assert!(text.contains("Lazygit"), "expanded items should be visible");

    // Fold every category: headers stay, member items disappear.
    let headers: Vec<String> = app
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::Header { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(!headers.is_empty(), "sample config should have categories");
    for name in &headers {
        let pos = app
            .rows
            .iter()
            .position(|r| matches!(r, Row::Header { name: n, .. } if n == name))
            .unwrap();
        assert!(app.toggle_category_at(pos));
    }
    let folded = buffer_text(&mut app);
    assert!(folded.contains("DEVELOP"), "headers survive folding");
    assert!(!folded.contains("Lazygit"), "members hidden when folded");
    // Only headers and uncategorized items remain.
    assert!(app
        .rows
        .iter()
        .all(|r| matches!(r, Row::Header { .. } | Row::Item(_))));
    assert_eq!(
        app.rows.iter().filter(|r| matches!(r, Row::Header { .. })).count(),
        headers.len()
    );
}

#[test]
fn selection_follows_the_row_under_it() {
    // Regression guard: the list is built from `rows`, so the index
    // the selection state holds must address the same row that gets
    // drawn — headers included.
    let mut app = test_app();
    for i in 0..app.rows.len() {
        app.state.select(Some(i));
        match &app.rows[i] {
            Row::Header { .. } => assert!(
                app.selected_item().is_none(),
                "row {i} is a header but resolved to an item"
            ),
            Row::Item(idx) => assert_eq!(
                app.selected_item().map(|it| it.label.as_str()),
                Some(app.items[*idx].label.as_str()),
                "row {i} resolved to the wrong item"
            ),
        }
    }
}

#[test]
fn search_flattens_categories_and_matches_fuzzily() {
    let mut app = test_app();
    app.mode = Mode::Search;
    app.query = "lzg".into();
    app.apply_filter();
    // No headers while searching, and the subsequence hit is found.
    assert!(app.rows.iter().all(|r| matches!(r, Row::Item(_))));
    assert_eq!(
        app.selected_item().map(|i| i.label.clone()),
        Some("Lazygit".into())
    );
    let text = buffer_text(&mut app);
    assert!(text.contains("/lzg"), "query echoes in the menu title");
}

#[test]
fn footer_shrinks_instead_of_truncating() {
    let mut app = test_app();
    // Wide: full config path fits.
    for (width, expect_path) in [(160u16, true), (110, false), (40, false)] {
        let mut terminal =
            ratatui::Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let last = buf.area.height - 1;
        let row: String = (0..buf.area.width)
            .map(|x| buf[(x, last)].symbol())
            .collect();
        assert!(
            row.trim().chars().count() <= width as usize,
            "footer overflows at width {width}"
        );
        assert_eq!(
            row.contains(&app.config_path),
            expect_path,
            "full path presence wrong at width {width}"
        );
        // The clock's seconds field must never be cut off the end.
        assert!(
            row.trim_end().ends_with(|c: char| c.is_ascii_digit()),
            "clock truncated at width {width}: {:?}",
            row.trim_end()
        );
    }
}

/// Reads the fg colours of a pane's border cells, walking clockwise
/// from the top-left corner. Each entry carries its distance along
/// the perimeter, because a title interrupts the run of border
/// glyphs and the cells either side of it are not neighbours.
/// Returns the menu pane's rect alongside its border colours. The
/// rect comes from what the app actually drew rather than being
/// hardcoded, so a layout change can't silently make these tests
/// sample interior cells instead of the border.
type Rgb = (u8, u8, u8);
/// A border cell: how far along the perimeter it sits, and the
/// colour it was rendered in.
type BorderCell = (u32, Rgb);

fn border_colors(app: &mut App) -> (Rect, Vec<BorderCell>) {
    let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let area = app.menu_area.expect("draw records the menu area");
    let buf = terminal.backend().buffer().clone();
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.x + area.width - 1, area.y + area.height - 1);
    let mut coords: Vec<(u16, u16)> = Vec::new();
    coords.extend((x0..=x1).map(|x| (x, y0)));
    coords.extend(((y0 + 1)..=y1).map(|y| (x1, y)));
    coords.extend((x0..x1).rev().map(|x| (x, y1)));
    coords.extend(((y0 + 1)..y1).rev().map(|y| (x0, y)));
    let colors = coords
        .into_iter()
        .enumerate()
        .filter(|&(_, (x, y))| is_border_glyph(buf[(x, y)].symbol()))
        .filter_map(|(pos, (x, y))| match buf[(x, y)].fg {
            Color::Rgb(r, g, b) => Some((pos as u32, (r, g, b))),
            _ => None,
        })
        .collect();
    (area, colors)
}

#[test]
fn rainbow_chase_is_a_smooth_travelling_gradient() {
    use crate::app::Theme;
    let mut app = test_app();
    app.theme = Theme::Rainbow;
    app.animate = true;

    let (area, colors) = border_colors(&mut app);
    assert!(colors.len() > 50, "expected a full border of cells");

    // Diffused, not banded: cells that really are adjacent stay
    // close in colour, all the way around including the corners.
    let step = |a: Rgb, b: Rgb| {
        (a.0 as i32 - b.0 as i32).abs().max(
            (a.1 as i32 - b.1 as i32)
                .abs()
                .max((a.2 as i32 - b.2 as i32).abs()),
        )
    };
    let adjacent: Vec<i32> = colors
        .windows(2)
        .filter(|w| w[1].0 == w[0].0 + 1)
        .map(|w| step(w[0].1, w[1].1))
        .collect();
    assert!(adjacent.len() > 40, "expected long unbroken runs of border");
    let biggest = *adjacent.iter().max().unwrap();
    assert!(
        biggest <= 20,
        "gradient should be gradual, but adjacent cells jumped by {biggest}"
    );

    // Rainbow, not monochrome: the whole wheel is represented.
    let distinct = colors
        .iter()
        .map(|(_, c)| c)
        .collect::<std::collections::HashSet<_>>();
    assert!(
        distinct.len() > 20,
        "expected many hues around the border, got {}",
        distinct.len()
    );

    // It travels: the same cells are lit differently a few ticks on.
    const TICKS: u64 = 12;
    for _ in 0..TICKS {
        app.on_tick();
    }
    let later = border_colors(&mut app).1;
    assert_ne!(colors, later, "the chase should move over time");

    // And it travels as a chase — the whole pattern slides clockwise
    // by a predictable distance rather than every cell recolouring
    // independently. After TICKS, the light at position p is what
    // used to be at p - shift.
    let perimeter = f32::from(2 * (area.width - 1) + 2 * (area.height - 1));
    let shift = (TICKS as f32 * app.chase_degrees_per_tick / 360.0 * perimeter)
        .round() as u32;
    assert!(shift > 0, "the test needs enough ticks to move the pattern");

    let earlier: std::collections::HashMap<u32, Rgb> =
        colors.iter().copied().collect();
    let mut compared = 0;
    let mut worst = 0;
    for (pos, c) in &later {
        let Some(prev) = pos.checked_sub(shift).and_then(|p| earlier.get(&p)) else {
            continue;
        };
        worst = worst.max(step(*c, *prev));
        compared += 1;
    }
    assert!(compared > 30, "expected plenty of overlap to compare");
    assert!(
        worst <= 25,
        "pattern should have slid {shift} cells clockwise, but a cell \
         differed from its predecessor by {worst}"
    );
}

#[test]
fn chase_lap_secs_sets_the_speed_and_rejects_nonsense() {
    use crate::app::{Theme, TICKS_PER_SEC};

    let lap_of = |configured: Option<f32>| {
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("bbs.toml");
        let (mut config, path) =
            crate::config::load_config(Some(config_path)).unwrap();
        config.bbs.chase_lap_secs = configured;
        let app = App::new(config, path);
        // Invert the conversion to recover the effective lap time.
        360.0 / (app.chase_degrees_per_tick * TICKS_PER_SEC)
    };

    let approx = |a: f32, b: f32| (a - b).abs() < 0.01;
    assert!(approx(lap_of(Some(4.0)), 4.0), "a plain value is honoured");
    assert!(approx(lap_of(None), 12.0), "unset falls back to the default");
    // Out-of-range and non-finite values clamp or fall back rather
    // than producing a strobe or a frozen border.
    assert!(approx(lap_of(Some(0.0)), 0.5), "too fast clamps up");
    assert!(approx(lap_of(Some(-3.0)), 0.5), "negative clamps up");
    assert!(approx(lap_of(Some(99_999.0)), 600.0), "too slow clamps down");
    assert!(approx(lap_of(Some(f32::NAN)), 12.0), "NaN falls back");
    assert!(approx(lap_of(Some(f32::INFINITY)), 12.0), "inf falls back");

    // A faster lap really does move the pattern further per tick.
    let sample = |lap: f32| {
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("bbs.toml");
        let (mut config, path) =
            crate::config::load_config(Some(config_path)).unwrap();
        config.bbs.chase_lap_secs = Some(lap);
        let mut app = App::new(config, path);
        app.theme = Theme::Rainbow;
        app.animate = true;
        let before = border_colors(&mut app).1;
        app.on_tick();
        let after = border_colors(&mut app).1;
        // Total colour movement across the strip after one tick.
        before
            .iter()
            .zip(after.iter())
            .map(|((_, a), (_, b))| {
                (a.0 as i32 - b.0 as i32).abs()
                    + (a.1 as i32 - b.1 as i32).abs()
                    + (a.2 as i32 - b.2 as i32).abs()
            })
            .sum::<i32>()
    };
    assert!(
        sample(2.0) > sample(60.0),
        "a shorter lap should advance the chase further each tick"
    );
}

#[test]
fn solid_themes_chase_a_dim_band_in_their_own_colour() {
    use crate::app::Theme;
    let mut app = test_app();
    app.theme = Theme::Solid(Color::Cyan);
    app.animate = true;

    let colors = border_colors(&mut app).1;
    assert!(colors.len() > 50, "expected a full border of cells");

    // One hue throughout: every cell is the theme colour at some
    // brightness, so normalising by the brightest channel gives the
    // same chromaticity everywhere.
    let chroma = |(r, g, b): Rgb| {
        let m = r.max(g).max(b).max(1) as f32;
        (r as f32 / m, g as f32 / m, b as f32 / m)
    };
    let first = chroma(colors[0].1);
    for (_, c) in &colors {
        let k = chroma(*c);
        let off = (k.0 - first.0)
            .abs()
            .max((k.1 - first.1).abs())
            .max((k.2 - first.2).abs());
        assert!(off < 0.05, "solid chase must not shift hue, saw {c:?}");
    }

    // But brightness does vary — that is the band.
    let level = |(r, g, b): Rgb| r as u32 + g as u32 + b as u32;
    let dimmest = colors.iter().map(|(_, c)| level(*c)).min().unwrap();
    let brightest = colors.iter().map(|(_, c)| level(*c)).max().unwrap();
    assert!(
        dimmest * 2 < brightest,
        "expected a clearly dim band ({dimmest} vs {brightest})"
    );

    // Mostly lit, with the darkness confined to a travelling band
    // rather than half the border.
    let midpoint = (dimmest + brightest) / 2;
    let lit = colors.iter().filter(|(_, c)| level(*c) > midpoint).count();
    assert!(
        lit * 2 > colors.len(),
        "the band should be narrower than the lit stretch"
    );

    // And it travels.
    let before = colors;
    for _ in 0..12 {
        app.on_tick();
    }
    assert_ne!(before, border_colors(&mut app).1, "the band should move");
}

/// The banner's colours, which is where its animation lives — the
/// glyphs themselves never change, so comparing rendered text would
/// miss it entirely.
fn banner_colors(app: &mut App) -> Vec<Color> {
    let mut terminal = ratatui::Terminal::new(TestBackend::new(110, 32)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..8u16)
        .flat_map(|y| (0..110u16).map(move |x| (x, y)))
        .filter(|&(x, y)| buf[(x, y)].symbol().trim() != "")
        .map(|(x, y)| buf[(x, y)].fg)
        .collect()
}

#[test]
fn animation_toggle_governs_banner_and_chase_together() {
    use crate::app::Theme;

    // Deliberately avoids the rendered text: the footer carries a
    // wall clock, so a text comparison would be flaky, and the
    // banner animates in colour rather than in glyphs anyway.
    let mut app = test_app();
    app.theme = Theme::Rainbow;

    // Frozen: ticks move neither the banner nor the chase.
    app.animate = false;
    let banner = banner_colors(&mut app);
    let borders = border_colors(&mut app).1;
    for _ in 0..30 {
        app.on_tick();
    }
    assert_eq!(banner, banner_colors(&mut app), "banner should be frozen");
    assert_eq!(borders, border_colors(&mut app).1, "chase should be frozen");

    // Running: both move.
    app.animate = true;
    let banner = banner_colors(&mut app);
    let borders = border_colors(&mut app).1;
    for _ in 0..30 {
        app.on_tick();
    }
    assert_ne!(banner, banner_colors(&mut app), "banner should animate");
    assert_ne!(borders, border_colors(&mut app).1, "chase should animate");
}

#[test]
fn border_chase_can_be_switched_off() {
    let mut app = test_app();
    app.theme = crate::app::Theme::Solid(Color::Cyan);
    app.chase = false;
    // With the chase off, borders keep their plain named colour and
    // no cell carries an Rgb fg.
    assert!(
        border_colors(&mut app).1.is_empty(),
        "no cell should be repainted when the chase is disabled"
    );
}

#[test]
fn marquee_wraps_around_and_handles_edges() {
    assert_eq!(marquee("abcd", 4, 0), "abcd");
    assert_eq!(marquee("abcd", 4, 1), "bcda");
    // Offsets past the end wrap instead of running out of text.
    assert_eq!(marquee("abcd", 4, 5), "bcda");
    // A window wider than the text repeats it.
    assert_eq!(marquee("ab", 5, 0), "ababa");
    assert_eq!(marquee("", 4, 0), "");
    assert_eq!(marquee("abcd", 0, 0), "");
}

#[test]
fn ticker_renders_and_is_hidden_without_motd() {
    let mut app = test_app();
    assert!(app.motd.is_some(), "sample config sets a motd");
    assert!(buffer_text(&mut app).contains("Welcome back"));

    // No motd -> no ticker row, and the menu still draws.
    app.motd = None;
    let text = buffer_text(&mut app);
    assert!(!text.contains("Welcome back"));
    assert!(text.contains("Main Menu"));
}

#[test]
fn github_screen_renders_with_entries() {
    let mut app = test_app();
    app.mode = Mode::Github;
    app.github.owner = Some("lnorton89".into());
    app.github.status = "connected as @lnorton89".into();
    let tab = app.github.tab;
    app.github.entries[tab].push(Entry {
        title: "Fix the bug".into(),
        subtitle: "octo/app · @octocat".into(),
        id: "#12".into(),
        url: Some("https://github.com/octo/app/pull/12".into()),
        detail: vec![("Repository".into(), "octo/app".into())],
        sort: None,
    });
    app.github.states[tab].select(Some(0));

    let text = buffer_text(&mut app);
    assert!(text.contains("GitHub Dashboard"));
    assert!(text.contains("Notifications"));
    assert!(text.contains("Fix the bug"));
    assert!(text.contains("Repository"));
}

#[test]
fn rainbow_theme_parses_and_cycles() {
    use crate::app::Theme;
    assert_eq!(Theme::parse("rainbow"), Theme::Rainbow);
    assert_eq!(Theme::parse("PRIDE"), Theme::Rainbow);
    assert_eq!(Theme::parse("cyan"), Theme::Solid(Color::Cyan));

    let mut app = test_app();
    app.theme = Theme::Rainbow;
    app.animate = true;
    let first = app.accent();
    for _ in 0..10 {
        app.on_tick();
    }
    let second = app.accent();
    assert_ne!(first, second, "hue should move with ticks");

    app.animate = false;
    let fixed = app.accent();
    assert_eq!(fixed, app.accent(), "static hue when animation off");

    // The full screen renders fine under a rainbow accent.
    let text = buffer_text(&mut app);
    assert!(text.contains("Main Menu"));
}

#[test]
fn bluetti_screen_renders_live_fields_and_summary() {
    use crate::screens::bluetti::Field;
    use std::time::Instant;

    let mut app = test_app();
    app.mode = Mode::Bluetti;
    app.bluetti.status = "connected to mqtt://127.0.0.1:1883".into();
    app.bluetti.connected = true;
    app.bluetti.devices.push("AC500-2237000003358".into());
    let fields = app
        .bluetti
        .fields
        .entry("AC500-2237000003358".into())
        .or_default();
    for (name, value) in [
        ("total_battery_percent", "33"),
        ("ac_output_power", "241"),
        ("ac_input_power", "0"),
        ("ac_output_on", "ON"),
        ("device_type", "AC500"),
    ] {
        fields.insert(
            name.into(),
            Field {
                value: value.into(),
                updated: Instant::now(),
            },
        );
    }
    app.bluetti.state.select(Some(0));

    let text = buffer_text(&mut app);
    assert!(text.contains("Bluetti Monitor"));
    assert!(text.contains("AC500-2237000003358"));
    assert!(text.contains("Battery"), "labels render");
    assert!(text.contains("241 W"), "units render");
    assert!(text.contains("241 W out"), "summary totals render");
    assert!(text.contains("connected to mqtt://127.0.0.1:1883"));

    // The battery gauge reflects the live percentage.
    assert!(text.contains("33%"));

    // Empty state: no devices yet still renders with a hint.
    let mut app = test_app();
    app.mode = Mode::Bluetti;
    let text = buffer_text(&mut app);
    assert!(text.contains("waiting for device data"));
    assert!(text.contains("no data yet"));
}

#[test]
fn sparkline_scales_windows_and_handles_flat_data() {
    use super::bluetti::sparkline;
    assert_eq!(sparkline(&[], 10), "");
    // Flat data sits mid-height rather than vanishing.
    assert_eq!(sparkline(&[5.0, 5.0, 5.0], 10), "▄▄▄");
    // A ramp spans the full glyph range.
    let ramp = sparkline(&[0.0, 50.0, 100.0], 10);
    assert!(ramp.starts_with('▁') && ramp.ends_with('█'), "{ramp}");
    // Only the last `width` samples are drawn.
    assert_eq!(sparkline(&[0.0, 100.0, 100.0], 2).chars().count(), 2);
    assert_eq!(sparkline(&[0.0, 100.0, 100.0], 2), "▄▄");
}

#[test]
fn github_screen_renders_error_state() {
    let mut app = test_app();
    app.mode = Mode::Github;
    app.github.errors[0] = Some("GitHub CLI (gh) not available".into());

    let text = buffer_text(&mut app);
    assert!(text.contains("GitHub CLI"));
}
