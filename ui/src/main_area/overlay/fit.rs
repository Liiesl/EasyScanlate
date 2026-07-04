use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use iced::{Font, Size};

use super::cache::{fit_key, FitKey, FIT_CACHE_CAP};
use super::text::{measure_text, LINE_HEIGHT};

const MIN_FONT_SIZE: f32 = 1.0;
const FIT_ITERATIONS: u32 = 14;

struct FitCacheEntry {
    content: String,
    size: f32,
    height: f32,
}

struct FitCache {
    entries: HashMap<FitKey, FitCacheEntry>,
    order: VecDeque<FitKey>,
}

fn with_fit_cache<R>(f: impl FnOnce(&mut FitCache) -> R) -> R {
    thread_local! {
        static CACHE: RefCell<Option<Box<dyn std::any::Any>>> = RefCell::new(None);
    }
    CACHE.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let cache: &mut FitCache = borrowed
            .get_or_insert_with(|| {
                Box::new(FitCache {
                    entries: HashMap::new(),
                    order: VecDeque::new(),
                })
            })
            .downcast_mut()
            .expect("fit cache holds an incompatible type");
        f(cache)
    })
}

/// Largest font size at which `text` fits inside `bounds` (word wrapping at box width).
pub(crate) fn fit_font_size(text: &str, font: Font, bounds: Size) -> f32 {
    fit_font_metrics(text, font, bounds).0
}

/// Like `fit_font_size`, also returning wrapped text height at fitted size.
pub(crate) fn fit_font_metrics(text: &str, font: Font, bounds: Size) -> (f32, f32) {
    if text.is_empty() || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return (MIN_FONT_SIZE, 0.0);
    }
    let key = fit_key(text, font, bounds);
    let cached = with_fit_cache(|cache| {
        cache
            .entries
            .get(&key)
            .filter(|entry| entry.content == text)
            .map(|entry| (entry.size, entry.height))
    });
    if let Some(metrics) = cached {
        return metrics;
    }
    let mut low = MIN_FONT_SIZE;
    let mut high = (bounds.width.max(bounds.height) * 2.0).max(MIN_FONT_SIZE);
    let mut fitted_height = 0.0;
    for _ in 0..FIT_ITERATIONS {
        let mid = (low + high) / 2.0;
        let measured = measure_text(text, font, mid, bounds.width);
        if measured.width <= bounds.width && measured.height <= bounds.height {
            low = mid;
            fitted_height = measured.height;
        } else {
            high = mid;
        }
    }
    let size = low;
    with_fit_cache(|cache| {
        if !cache.entries.contains_key(&key) {
            if cache.entries.len() >= FIT_CACHE_CAP {
                if let Some(evicted) = cache.order.pop_front() {
                    cache.entries.remove(&evicted);
                }
            }
            cache.order.push_back(key);
        }
        cache.entries.insert(
            key,
            FitCacheEntry {
                content: text.to_owned(),
                size,
                height: fitted_height,
            },
        );
    });
    (size, fitted_height)
}
