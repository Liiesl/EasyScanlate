//! Small color helpers for the app's ui crate.

use iced::Color;

/// Converts an RGBA byte color to an iced [`Color`].
pub fn rgba_to_color(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3] as f32 / 255.0)
}

/// The uppercase hex value of `color`, or "None" for fully transparent
/// colors (matching the mockup's background swatch). Used by the styling
/// panel's hex inputs: `a==0` → `"None"`, `a==255` → `"#RRGGBB"`,
/// otherwise `"#RRGGBBAA"`. Kept here so app handlers and widgets share the
/// same canonical display.
pub fn hex_label(color: Color) -> String {
    let [r, g, b, a] = color.into_rgba8();
    if a == 0 {
        "None".to_string()
    } else if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// Parses a hex string into a `Color`, accepting the picker's short and long
/// forms plus the mockup's `"None"` (transparent). Trims whitespace, strips
/// an optional leading `#`, case-insensitive. Accepted digit lengths:
/// `3` (`RGB`→`RRGGBB`), `4` (`RGBA`), `6` (`RRGGBB`), `8` (`RRGGBBAA`).
/// Short forms expand each nibble (e.g. `"f80"`→`0xFF8800`). Missing alpha
/// defaults to `255`. Returns `None` for any other length or non-hex digits.
///
/// `Some(Color::TRANSPARENT)` is returned for `"None"` / `"none"` / `""`
/// where the styling panel wants fully transparent. For strict hex-only
/// callers, check `is_valid_hex` before.
pub fn parse_hex_color(s: &str) -> Option<Color> {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Some(Color::from_rgba8(0, 0, 0, 0.0));
    }
    let s = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if s.is_empty() {
        return None;
    }
    parse_hex_digits(s).map(|(r, g, b, a)| Color::from_rgba8(r, g, b, a as f32 / 255.0))
}

/// Returns `true` if `s` (with optional `#` and surrounding whitespace) is a
/// valid hex color or `"None"`. Used to decide when a typed hex buffer can be
/// live-applied.
pub fn is_valid_hex_input(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return true;
    }
    let s = trimmed.strip_prefix('#').unwrap_or(trimmed);
    parse_hex_digits(s).is_some()
}

/// Parses hex digits (already stripped `#`) into `(r,g,b,a)`. Duplicates the
/// `neverliie_iced_widgets::color_picker::color::parse_hex_digits` logic so
/// the ui crate has no internal dep on the picker's private module.
fn parse_hex_digits(s: &str) -> Option<(u8, u8, u8, u8)> {
    let digits: Vec<char> = s.chars().collect();
    let expand = matches!(digits.len(), 3 | 4);
    if !matches!(digits.len(), 3 | 4 | 6 | 8) || !digits.iter().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |c: char| c.to_digit(16).unwrap_or(0) as u8;
    let byte = |hi: char, lo: char| (nibble(hi) << 4) | nibble(lo);
    let (r, g, b) = if expand {
        (
            byte(digits[0], digits[0]),
            byte(digits[1], digits[1]),
            byte(digits[2], digits[2]),
        )
    } else {
        (
            byte(digits[0], digits[1]),
            byte(digits[2], digits[3]),
            byte(digits[4], digits[5]),
        )
    };
    let a = match digits.len() {
        3 | 6 => 255,
        4 => byte(digits[3], digits[3]),
        _ => byte(digits[6], digits[7]),
    };
    Some((r, g, b, a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_color_maps_channels_and_alpha() {
        assert_eq!(
            rgba_to_color([10, 20, 30, 255]),
            Color::from_rgba8(10, 20, 30, 1.0)
        );
        assert_eq!(
            rgba_to_color([255, 0, 128, 128]),
            Color::from_rgba8(255, 0, 128, 128.0 / 255.0)
        );
    }

    #[test]
    fn hex_label_formats_none_and_rgb_and_rgba() {
        assert_eq!(hex_label(Color::from_rgba8(0, 0, 0, 0.0)), "None");
        assert_eq!(hex_label(Color::from_rgb8(0x12, 0x34, 0x56)), "#123456");
        assert_eq!(
            hex_label(Color::from_rgba8(0x12, 0x34, 0x56, 0x80 as f32 / 255.0)),
            "#12345680"
        );
    }

    #[test]
    fn parse_hex_accepts_all_lengths_and_none() {
        // 6-digit
        assert!(parse_hex_color("#FF8000").is_some());
        assert!(parse_hex_color("FF8000").is_some());
        // 3-digit short
        assert_eq!(
            parse_hex_color("#F80").unwrap().into_rgba8(),
            Color::from_rgb8(0xFF, 0x88, 0x00).into_rgba8()
        );
        // 8-digit
        assert!(parse_hex_color("#FF800080").is_some());
        // 4-digit short with alpha
        assert!(parse_hex_color("#F80A").is_some());
        // None
        assert_eq!(
            parse_hex_color("None").unwrap().into_rgba8()[3],
            0
        );
        assert_eq!(
            parse_hex_color("none").unwrap().into_rgba8()[3],
            0
        );
        // invalid
        assert!(parse_hex_color("#12").is_none());
        assert!(parse_hex_color("GGGGGG").is_none());
        assert!(parse_hex_color("").is_none());
    }

    #[test]
    fn is_valid_hex_input_mirrors_parse() {
        assert!(is_valid_hex_input("#ff8000"));
        assert!(is_valid_hex_input("None"));
        assert!(!is_valid_hex_input("#12"));
        assert!(!is_valid_hex_input("zzzz"));
    }
}