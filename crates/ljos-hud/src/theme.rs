//! Seat mocha as icedtea tokens.

use iced::Color;
use icedtea::m3::{Density, DensityName, ElevationPolicy, ShapePolicy};
use icedtea::theme::Tokens;

pub use icedtea::typo::BODY as SIZE_BODY;
pub use icedtea::typo::META as SIZE_META;
pub use icedtea::typo::TITLE as SIZE_TITLE;
pub use icedtea::typo::UI as FACE;

pub const MANTLE: Color = Color::from_rgb8(0x18, 0x18, 0x25);
pub const BASE: Color = Color::from_rgb8(0x1e, 0x1e, 0x2e);
pub const SURFACE0: Color = Color::from_rgb8(0x31, 0x32, 0x44);
pub const TEXT: Color = Color::from_rgb8(0xcd, 0xd6, 0xf4);
pub const SUBTEXT: Color = Color::from_rgb8(0xa6, 0xad, 0xc8);
pub const BLUE: Color = Color::from_rgb8(0x89, 0xb4, 0xfa);
pub const GREEN: Color = Color::from_rgb8(0xa6, 0xe3, 0xa1);
pub const PEACH: Color = Color::from_rgb8(0xfa, 0xb3, 0x87);

/// Mocha mapped onto icedtea semantic tokens.
pub fn tokens() -> Tokens {
    Tokens::from_aliases(
        BASE,
        SURFACE0,
        MANTLE,
        TEXT,
        SUBTEXT,
        BLUE,
        PEACH,
        GREEN,
        Color::from_rgb8(0xf9, 0xe2, 0xaf),
        Color::from_rgb8(0xf3, 0x8b, 0xa8),
        Color::from_rgb8(0x58, 0x5b, 0x70),
    )
    .with_density(Density::named(DensityName::Compact))
    .with_shape(ShapePolicy::Material)
    .with_elevation(ElevationPolicy::Flat)
}

/// iced theme from the mocha tokens.
pub fn theme() -> iced::Theme {
    icedtea::theme::iced_theme("ljos-mocha", tokens())
}

/// WCAG contrast ratio of two sRGB colours.
pub fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (l1, l2) = (rel_lum(a), rel_lum(b));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

fn rel_lum(c: Color) -> f32 {
    fn lin(ch: f32) -> f32 {
        if ch <= 0.04045 {
            ch / 12.92
        } else {
            ((ch + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mocha_tokens_match_seat_aliases() {
        let t = tokens();
        assert_eq!(t.canvas, BASE);
        assert_eq!(t.surface, SURFACE0);
        assert_eq!(t.panel, MANTLE);
        assert_eq!(t.primary, BLUE);
    }

    #[test]
    fn focus_ring_meets_three_to_one_on_base() {
        assert!(
            contrast_ratio(BLUE, BASE) >= 3.0,
            "focus ring {} on base",
            contrast_ratio(BLUE, BASE)
        );
        assert!(contrast_ratio(TEXT, BASE) >= 3.0);
    }
}
