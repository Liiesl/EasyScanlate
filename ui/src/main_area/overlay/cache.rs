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
