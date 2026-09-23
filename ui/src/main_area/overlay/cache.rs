use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use iced::Font;

/// Upper bound on the number of entries in the shared fit cache.
pub const FIT_CACHE_CAP: usize = 2048;

pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub type FitKey = (u64, u32, u32, u64);

pub fn fnv1a(content: &str) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub fn font_hash(font: Font) -> u64 {
    let mut hasher = DefaultHasher::new();
    font.hash(&mut hasher);
    hasher.finish()
}

pub fn fit_key(text: &str, font: Font, bounds: iced::Size) -> FitKey {
    (
        fnv1a(text),
        bounds.width.to_bits(),
        bounds.height.to_bits(),
        font_hash(font),
    )
}

/// Mixes the relative line-height multiplier and the letter spacing into a
/// text hash so cached fits stay distinct across style values.
pub fn text_key(text: &str, line_height: f32, letter_spacing: f32) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    for word in [line_height.to_bits(), letter_spacing.to_bits()] {
        for byte in word.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

/// Like [`fit_key`], but distinct per line-height / letter-spacing pair.
pub fn fit_key_params(
    text: &str,
    font: Font,
    bounds: iced::Size,
    line_height: f32,
    letter_spacing: f32,
) -> FitKey {
    (
        text_key(text, line_height, letter_spacing),
        bounds.width.to_bits(),
        bounds.height.to_bits(),
        font_hash(font),
    )
}
