//! Theme colours and the travelling border-chase light shared by every
//! drawing surface.

use crate::app::{App, Theme};
use ratatui::{layout::Rect, style::Color, Frame};

/// Rounds `v` to the nearest multiple of `step`.
///
/// Animated colours are quantized in time with this: a cell's colour
/// only changes when the underlying value crosses a step, so between
/// steps the renderer's diff for that cell is empty and no bytes reach
/// the terminal. Without it every animated cell changed every frame,
/// and that sustained full-coverage recolouring is what pushed
/// ConPTY/Windows Terminal into progressive display corruption.
pub fn quant(v: f32, step: f32) -> f32 {
    (v / step).round() * step
}

/// Convert an HSV triple (h: 0-360, s/v: 0-1) to an RGB tuple.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match ((h % 360.0) as i32).max(0) {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// True for the box-drawing glyphs ratatui uses to stroke borders. The
/// chase recolours only these, so titles and content keep their own
/// colours instead of being swept up in the gradient.
pub(crate) fn is_border_glyph(symbol: &str) -> bool {
    symbol
        .chars()
        .next()
        .is_some_and(|c| ('\u{2500}'..='\u{257F}').contains(&c))
}

/// What the travelling border light is made of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChaseStyle {
    /// Sweep the full hue wheel around the outline.
    Hue,
    /// Hold one colour and chase a dim band through it.
    DimBand(u8, u8, u8),
}

/// Paints a travelling light around the border of `area`, like an LED
/// strip. Position along the perimeter drives the effect and the whole
/// pattern drifts with time, so it reads as light moving clockwise
/// rather than as cells blinking independently.
///
/// Under [`ChaseStyle::Hue`] one full wheel is spread over the outline,
/// so adjacent cells differ by only a degree or two, with a gentle
/// brightness wave riding along to give the motion something to show
/// even where neighbouring hues are nearly identical. Under
/// [`ChaseStyle::DimBand`] the hue is fixed and a narrow dimmed segment
/// travels through it instead.
///
/// `degrees_per_tick` sets the speed; it comes from the configured lap
/// time (see `chase_lap_secs`).
fn border_chase(
    frame: &mut Frame,
    area: Rect,
    tick: u64,
    animate: bool,
    degrees_per_tick: f32,
    style: ChaseStyle,
) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let (w, h) = (area.width as u32, area.height as u32);
    let perimeter = (2 * (w - 1) + 2 * (h - 1)) as f32;
    let phase_deg = if animate {
        tick as f32 * degrees_per_tick
    } else {
        0.0
    };
    let phase_rad = phase_deg.to_radians();

    let buf = frame.buffer_mut();
    let mut paint = |x: u16, y: u16, pos: u32| {
        let Some(cell) = buf.cell_mut((x, y)) else {
            return;
        };
        if !is_border_glyph(cell.symbol()) {
            return;
        }
        let t = pos as f32 / perimeter;
        let wave = (t * std::f32::consts::TAU - phase_rad).sin();
        // Quantized per cell: each cell repaints only when its own value
        // crosses a step, so per frame only the pattern's moving edges
        // emit bytes while the rest of the strip diffs to nothing. The
        // motion still reads as continuous because different cells sit
        // at different fractions of a step and advance at different
        // moments.
        let color = match style {
            ChaseStyle::Hue => {
                let hue = quant((t * 360.0 - phase_deg).rem_euclid(360.0), 4.0);
                // Stays well clear of 0 so the dim part of the wave
                // still reads as coloured light rather than going muddy.
                let glow = quant(0.68 + 0.32 * wave, 1.0 / 24.0);
                let (r, g, b) = hsv_to_rgb(hue, 0.85, glow);
                Color::Rgb(r, g, b)
            }
            ChaseStyle::DimBand(r, g, b) => {
                // Raising the normalised wave to a fractional power
                // pushes most of the lap up near full brightness, so the
                // dark part stays a narrow band travelling through the
                // theme colour instead of an even half-lit/half-dark
                // split. The floor keeps the band visible rather than
                // punching a hole in the border.
                let lit = (0.5 + 0.5 * wave).powf(0.4);
                let level = quant(DIM_FLOOR + (1.0 - DIM_FLOOR) * lit, 1.0 / 24.0);
                Color::Rgb(
                    (r as f32 * level) as u8,
                    (g as f32 * level) as u8,
                    (b as f32 * level) as u8,
                )
            }
        };
        cell.set_fg(color);
    };

    // Walk the outline clockwise from the top-left so `pos` measures
    // distance travelled along the strip.
    let (x0, y0) = (area.x, area.y);
    let (x1, y1) = (area.x + area.width - 1, area.y + area.height - 1);
    let mut pos = 0;
    for x in x0..=x1 {
        paint(x, y0, pos);
        pos += 1;
    }
    for y in (y0 + 1)..=y1 {
        paint(x1, y, pos);
        pos += 1;
    }
    for x in (x0..x1).rev() {
        paint(x, y1, pos);
        pos += 1;
    }
    for y in ((y0 + 1)..y1).rev() {
        paint(x0, y, pos);
        pos += 1;
    }
}

/// How far the dim band drops below the theme colour at its darkest.
const DIM_FLOOR: f32 = 0.3;

/// Applies the travelling border light to every bordered pane, in
/// whichever form suits the active theme. A no-op when the chase is
/// switched off.
pub(crate) fn apply_chase(frame: &mut Frame, app: &App, areas: &[Rect]) {
    if !app.chase {
        return;
    }
    let style = match app.theme {
        Theme::Rainbow => ChaseStyle::Hue,
        Theme::Solid(color) => {
            let (r, g, b) = theme_rgb(color);
            ChaseStyle::DimBand(r, g, b)
        }
    };
    for area in areas {
        border_chase(
            frame,
            *area,
            app.tick,
            app.animate,
            app.chase_degrees_per_tick,
            style,
        );
    }
}

pub fn color_from_str(s: &str) -> Color {
    match s.to_lowercase().as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::White,
    }
}

/// RGB base used for the banner shimmer gradient of each theme color.
pub(crate) fn theme_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Red | Color::LightRed => (255, 85, 85),
        Color::Green | Color::LightGreen => (80, 250, 123),
        Color::Yellow | Color::LightYellow => (241, 250, 140),
        Color::Blue | Color::LightBlue => (98, 114, 250),
        Color::Magenta | Color::LightMagenta => (255, 121, 198),
        Color::White | Color::Gray => (245, 245, 245),
        // Rainbow theme: use the current animated hue directly.
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 220, 255),
    }
}
