//! iced widgets. Everything here reads the app through [`UiState`] and emits
//! [`UiEvent`]s; the ui crate never touches the app crate.
//!
//! [`UiState`]: crate::state::UiState
//! [`UiEvent`]: crate::event::UiEvent

pub mod event;
pub mod loaded;
pub mod main_area;
pub mod panel;
pub mod state;

pub use loaded::LoadedImage;
pub use state::UiState;

pub const KOREAN_FONT_PATH: &str = "C:\\Windows\\Fonts\\malgun.ttf";
pub const KOREAN_FONT_NAME: &str = "Malgun Gothic";

/// Parses `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA` into an RGBA color.
/// Shorthand forms expand the alpha to `255`.
pub fn parse_hex(text: &str) -> Option<[u8; 4]> {
    let digits: Vec<u8> = text
        .strip_prefix('#')
        .and_then(|rest| {
            (rest.len() == 3 || rest.len() == 4 || rest.len() == 6 || rest.len() == 8)
                .then_some(rest)
        })?
        .bytes()
        .map(|b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        })
        .collect::<Option<Vec<u8>>>()?;
    let (short, chars) = match digits.len() {
        3 | 4 => (true, digits.len()),
        6 | 8 => (false, digits.len() / 2),
        _ => return None,
    };
    let mut out = [0u8; 4];
    for i in 0..4 {
        let value = if i < chars {
            if short {
                digits[i] * 17
            } else {
                digits[i * 2] * 16 + digits[i * 2 + 1]
            }
        } else {
            255
        };
        out[i] = value;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        for color in [
            [0, 0, 0, 255],
            [255, 230, 90, 255],
            [20, 20, 31, 140],
        ] {
            assert_eq!(parse_hex(&format!("#{:02X}{:02X}{:02X}{:02X}", color[0], color[1], color[2], color[3])), Some(color));
        }
    }

    #[test]
    fn hex_parses_shorthand_with_alpha_default() {
        assert_eq!(parse_hex("#FFF"), Some([255, 255, 255, 255]));
        assert_eq!(parse_hex("#fff0"), Some([255, 255, 255, 0]));
        assert_eq!(parse_hex("#FFE65A"), Some([255, 230, 90, 255]));
    }

    #[test]
    fn hex_rejects_malformed_input() {
        for bad in ["", "#", "#12", "#GGG", "#12345", "FFE65A", "red", "#123456789"] {
            assert_eq!(parse_hex(bad), None, "expected {bad:?} to be rejected");
        }
    }
}