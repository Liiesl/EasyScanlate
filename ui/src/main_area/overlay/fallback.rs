//! CJK script detection and OS fallback family list for issue #24.
//!
//! `Anime Ace` and `augie` are Latin-only decorative fonts. When an entry
//! contains Hangul/Han/Kana and requests `Bold`/`Italic`, cosmic-text's
//! fallback scanning fails (the fallback CJK font only has `Normal`) and
//! either shows `□` or spends 14× shaping passes scanning the DB. The fix is
//! twofold: preload OS CJK fonts into the iced `FontSystem` (see
//! `src/app/boot.rs`) and degrade `Bold`/`Italic` to `Normal` for CJK
//! content at the call site (`style::styled_font_for_text`).

/// Families that provide CJK coverage on at least one major OS. The boot
/// task filters `fontdb` by this list (case-insensitive) and loads the
/// matching files into `cosmic_text` via `iced::font::load`. Keep this
/// sorted for readability; matching is `eq_ignore_ascii_case`.
pub const CJK_FALLBACK_FAMILIES: &[&str] = &[
    // Windows
    "Batang",
    "Dotum",
    "Gulim",
    "Malgun Gothic",
    "Meiryo",
    "MS Gothic",
    "MS Mincho",
    "MS PGothic",
    "Yu Gothic",
    "Yu Mincho",
    // macOS
    "Apple SD Gothic Neo",
    "AppleGothic",
    "Hiragino Kaku Gothic Pro",
    "Hiragino Kaku Gothic ProN",
    "Hiragino Mincho ProN",
    "Hiragino Sans",
    "Hiragino Sans GB",
    // Linux / Noto
    "Nanum Gothic",
    "Noto Sans CJK JP",
    "Noto Sans CJK KR",
    "Noto Sans CJK SC",
    "Noto Sans JP",
    "Noto Sans KR",
    "Noto Sans SC",
    // Generic CJK aliases that appear via fontconfig
    "Noto Sans CJK",
];

/// Returns true if `family` is one of the CJK fallback families
/// (case-insensitive).
pub fn is_cjk_fallback_family(family: &str) -> bool {
    CJK_FALLBACK_FAMILIES
        .iter()
        .any(|f| f.eq_ignore_ascii_case(family))
}

/// Quick check for a single codepoint belonging to CJK / Hangul / Kana
/// blocks. Covers the ranges that appear in manhwa/manga.
pub fn is_cjk(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        // Hangul Jamo
        0x1100..=0x11FF
        // Hangul Compatibility Jamo
        | 0x3130..=0x318F
        // Hangul Jamo Extended-A/B
        | 0xA960..=0xA97F
        | 0xD7B0..=0xD7FF
        // Hangul Syllables
        | 0xAC00..=0xD7AF
        // CJK Symbols, Hiragana, Katakana, Bopomofo
        | 0x3000..=0x303F
        | 0x3040..=0x309F
        | 0x30A0..=0x30FF
        | 0x3100..=0x312F
        | 0x31A0..=0x31BF
        | 0x31F0..=0x31FF
        // CJK Unified Ideographs (common + Extension A)
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        // CJK Compatibility Ideographs
        | 0xF900..=0xFAFF
        // CJK Compatibility Forms, Halfwidth
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF
        // Extension B and beyond (supplementary) — check high range
        | 0x20000..=0x2FA1F
    )
}

/// Returns true if `text` contains at least one CJK codepoint.
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(is_cjk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_korean() {
        assert!(contains_cjk("안녕하세요!"));
        assert!("안".chars().all(is_cjk));
    }

    #[test]
    fn detects_japanese() {
        assert!(contains_cjk("こんにちは"));
        assert!(contains_cjk("カタカナ"));
        assert!(contains_cjk("漢字"));
    }

    #[test]
    fn detects_chinese() {
        assert!(contains_cjk("你好"));
    }

    #[test]
    fn not_cjk_for_latin() {
        assert!(!contains_cjk("Hello World!"));
        assert!(!contains_cjk("Anime Ace 123"));
    }

    #[test]
    fn mixed_is_cjk() {
        assert!(contains_cjk("Hello 안녕"));
    }
}
