use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use iced::{Font, Size};

use super::cache::{fit_key, FitKey, FIT_CACHE_CAP};
use super::text::{line_fits, measure_text, LINE_HEIGHT};

/// One laid-out line of a circular bubble.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CircleLine {
    pub content: String,
    pub y: f32,
    pub chord: f32,
}

struct CircleFitCacheEntry {
    content: String,
    size: f32,
    lines: Vec<CircleLine>,
}

struct CircleFitCache {
    entries: HashMap<FitKey, CircleFitCacheEntry>,
    order: VecDeque<FitKey>,
}

fn with_circle_cache<R>(f: impl FnOnce(&mut CircleFitCache) -> R) -> R {
    thread_local! {
        static CACHE: RefCell<Option<Box<dyn std::any::Any>>> = RefCell::new(None);
    }
    CACHE.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let cache: &mut CircleFitCache = borrowed
            .get_or_insert_with(|| {
                Box::new(CircleFitCache {
                    entries: HashMap::new(),
                    order: VecDeque::new(),
                })
            })
            .downcast_mut()
            .expect("circle fit cache holds an incompatible type");
        f(cache)
    })
}

const MIN_FONT_SIZE: f32 = 1.0;
const FIT_ITERATIONS: u32 = 14;

pub(crate) fn chord_at(rx: f32, ry: f32, yc: f32) -> f32 {
    let t = 1.0 - ((yc - ry) / ry).powi(2);
    if t <= 0.0 {
        0.0
    } else {
        2.0 * rx * t.sqrt()
    }
}

fn chord_at_centered(rx: f32, ry: f32, yc: f32) -> f32 {
    chord_at(rx, ry, yc)
}

fn circle_tokens(text: &str, font: Font, size: f32, max_width: f32) -> Vec<String> {
    let mut tokens = Vec::new();
    for word in text.split_whitespace() {
        if measure_text(word, font, size, f32::INFINITY).width <= max_width {
            tokens.push(word.to_string());
            continue;
        }
        let mut sub = String::new();
        for ch in word.chars() {
            let candidate = {
                let mut c = sub.clone();
                c.push(ch);
                c
            };
            let cand_width = measure_text(&candidate, font, size, f32::INFINITY).width;
            if !sub.is_empty() && cand_width > max_width {
                tokens.push(std::mem::take(&mut sub));
                sub.push(ch);
            } else {
                sub = candidate;
            }
        }
        if !sub.is_empty() {
            tokens.push(sub);
        }
    }
    tokens
}

fn layout_circle_lines(text: &str, font: Font, size: f32, bounds: Size) -> Option<Vec<CircleLine>> {
    let rx = bounds.width / 2.0;
    let ry = bounds.height / 2.0;
    let line_height = size * LINE_HEIGHT;
    if line_height <= 0.0 {
        return None;
    }
    // Split hard breaks on '\n' first, keeping empty segments for blank gaps.
    // Each segment is tokenized independently; empty segments produce a blank line.
    let paras: Vec<Vec<String>> = text
        .split('\n')
        .map(|seg| {
            if seg.trim().is_empty() {
                Vec::new()
            } else {
                circle_tokens(seg, font, size, bounds.width)
            }
        })
        .collect();

    // Pure whitespace without newline -> no visible lines (match previous behaviour).
    if paras.iter().all(|p| p.is_empty()) && !text.contains('\n') {
        return Some(Vec::new());
    }

    let max_lines = (bounds.height / line_height).floor() as usize;
    if max_lines == 0 {
        return None;
    }
    for n in 1..=max_lines {
        let chords: Vec<f32> = (0..n)
            .map(|i| {
                let yc = ry + (i as f32 - (n as f32 - 1.0) / 2.0) * line_height;
                chord_at_centered(rx, ry, yc)
            })
            .collect();
        let mut lines: Vec<CircleLine> = Vec::with_capacity(n);
        let mut para_idx = 0usize;
        let mut tok_idx = 0usize;
        let mut ok = true;
        for (i, &chord) in chords.iter().enumerate() {
            if para_idx >= paras.len() {
                break;
            }
            // Empty paragraph => blank gap line (respect newline). Chord size not checked for empty.
            if paras[para_idx].is_empty() {
                lines.push(CircleLine {
                    content: String::new(),
                    y: i as f32 * line_height,
                    chord,
                });
                para_idx += 1;
                tok_idx = 0;
                continue;
            }
            if chord <= 1.0 {
                ok = false;
                break;
            }
            let mut content = String::new();
            while tok_idx < paras[para_idx].len() {
                let candidate = if content.is_empty() {
                    paras[para_idx][tok_idx].clone()
                } else {
                    format!("{} {}", content, paras[para_idx][tok_idx])
                };
                if line_fits(&candidate, font, size, chord) {
                    content = candidate;
                    tok_idx += 1;
                } else if content.is_empty() {
                    ok = false;
                    break;
                } else {
                    break;
                }
            }
            if !ok {
                break;
            }
            lines.push(CircleLine {
                content,
                y: i as f32 * line_height,
                chord,
            });
            // Hard break: if this paragraph is fully consumed, advance to next paragraph
            // for the next chord (never merge across '\n').
            if tok_idx >= paras[para_idx].len() {
                para_idx += 1;
                tok_idx = 0;
            }
            if para_idx >= paras.len() {
                // All paragraphs consumed; remaining chords (if any) are not needed.
                // Break early to keep lines.len() minimal (original behaviour).
                break;
            }
        }
        if !ok {
            continue;
        }
        if para_idx >= paras.len() {
            return Some(lines);
        }
    }
    None
}

/// Largest font size at which `text` fits `bounds` as circular bubble.
pub(crate) fn fit_circle_metrics(text: &str, font: Font, bounds: Size) -> (f32, Vec<CircleLine>) {
    if text.is_empty() || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return (MIN_FONT_SIZE, Vec::new());
    }
    let key = fit_key(text, font, bounds);
    let cached = with_circle_cache(|cache| {
        cache
            .entries
            .get(&key)
            .filter(|entry| entry.content == text)
            .map(|entry| (entry.size, entry.lines.clone()))
    });
    if let Some(metrics) = cached {
        return metrics;
    }
    let mut low = MIN_FONT_SIZE;
    let mut high = (bounds.width.max(bounds.height) * 2.0).max(MIN_FONT_SIZE);
    let mut best: Vec<CircleLine> = Vec::new();
    for _ in 0..FIT_ITERATIONS {
        let mid = (low + high) / 2.0;
        match layout_circle_lines(text, font, mid, bounds) {
            Some(lines) => {
                low = mid;
                best = lines;
            }
            None => high = mid,
        }
    }
    let size = low;
    let lines = if best.is_empty() {
        layout_circle_lines(text, font, MIN_FONT_SIZE, bounds).unwrap_or_default()
    } else {
        best
    };
    with_circle_cache(|cache| {
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
            CircleFitCacheEntry {
                content: text.to_owned(),
                size,
                lines: lines.clone(),
            },
        );
    });
    (size, lines)
}
