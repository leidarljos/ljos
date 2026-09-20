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
