//! Hand-rolled block-letter ASCII art banners for terminal apps.
//!
//! No external font files and no reliance on a terminal's particular
//! fallback rendering of Unicode block glyphs — every style here is either
//! plain box-drawing characters or a documented, deliberate substitution
//! over them, so output looks the same everywhere it's printed.
//!
//! ```
//! let banner = blockfont::render("HI", blockfont::Style::Shadow);
//! assert_eq!(banner.lines().count(), 6);
//! ```

mod glyphs;

use std::fmt;
use std::str::FromStr;

/// Which block-letter style to render text in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// Double-line "ANSI Shadow" style: solid `█` fill with `╗ ╔ ╝ ╚ ═ ║`
    /// double-line shadows.
    #[default]
    Shadow,
    /// Same letterforms as [`Style::Shadow`], but the solid fill is swapped
    /// for a horizontal-line stroke (`═`) instead of a solid block.
    Lined,
}

/// Returned by [`Style::from_str`] when the input doesn't match a known style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseStyleError(String);

impl fmt::Display for ParseStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown blockfont style: {:?}", self.0)
    }
}

impl std::error::Error for ParseStyleError {}

impl FromStr for Style {
    type Err = ParseStyleError;

    /// Case-insensitive. `Lined` accepts a couple of common aliases.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "shadow" => Ok(Style::Shadow),
            "lined" | "lines" | "hatch" | "striped" => Ok(Style::Lined),
            other => Err(ParseStyleError(other.to_string())),
        }
    }
}

/// Fill character used for "solid" cells in [`Style::Lined`]: the same
/// double-line box-drawing stroke (`═`) already used for the border
/// shadows, so a run of filled cells reads as one continuous horizontal
/// line instead of a solid block — and stays visually consistent with the
/// rest of the glyph instead of introducing an unrelated symbol.
const LINED_FILL: &str = "═";

/// Renders `text` as block-letter ASCII art in the given `style`, with a
/// single column of space between letters. Input is uppercase-normalized;
/// spaces and unrecognized characters render as a blank cell. Returns
/// newline-joined rows, every row the same display width.
pub fn render(text: &str, style: Style) -> String {
    match style {
        Style::Shadow => render_with(text, |c| glyphs::shadow(c).map(String::from)),
        Style::Lined => render_with(text, |c| {
            glyphs::shadow(c).map(|row| row.replace('█', LINED_FILL))
        }),
    }
}

fn render_with<const H: usize>(text: &str, glyph_rows: impl Fn(char) -> [String; H]) -> String {
    let mut lines = vec![String::new(); H];
    for (i, c) in text.chars().enumerate() {
        let glyph = glyph_rows(c);
        for (row, line) in lines.iter_mut().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            line.push_str(&glyph[row]);
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_six_equal_width_rows() {
        let banner = render("HI", Style::Shadow);
        let rows: Vec<&str> = banner.lines().collect();
        assert_eq!(rows.len(), 6);
        let width = rows[0].chars().count();
        assert!(rows.iter().all(|r| r.chars().count() == width));
    }

    #[test]
    fn lined_has_no_solid_fill() {
        let banner = render("HI", Style::Lined);
        assert!(!banner.contains('█'));
    }

    #[test]
    fn style_from_str_accepts_aliases_and_rejects_unknown() {
        assert_eq!("shadow".parse::<Style>().unwrap(), Style::Shadow);
        assert_eq!("Lined".parse::<Style>().unwrap(), Style::Lined);
        assert_eq!("striped".parse::<Style>().unwrap(), Style::Lined);
        assert!("nonsense".parse::<Style>().is_err());
    }
}
