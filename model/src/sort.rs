// Natural ordering for file paths: numeric chunks compared as numbers
// so `2.jpg < 10.jpg < 11.jpg` instead of lexical `10 < 11 < 2`.
use std::cmp::Ordering;

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        match (a_chars.peek(), b_chars.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ac), Some(&bc)) => {
                let a_digit = ac.is_ascii_digit();
                let b_digit = bc.is_ascii_digit();
                if a_digit && b_digit {
                    let mut a_num = String::new();
                    while let Some(&c) = a_chars.peek() {
                        if c.is_ascii_digit() { a_num.push(c); a_chars.next(); } else { break; }
                    }
                    let mut b_num = String::new();
                    while let Some(&c) = b_chars.peek() {
                        if c.is_ascii_digit() { b_num.push(c); b_chars.next(); } else { break; }
                    }
                    let a_trim = a_num.trim_start_matches('0');
                    let b_trim = b_num.trim_start_matches('0');
                    let a_trim = if a_trim.is_empty() { "0" } else { a_trim };
                    let b_trim = if b_trim.is_empty() { "0" } else { b_trim };
                    match a_trim.len().cmp(&b_trim.len()) {
                        Ordering::Equal => match a_trim.cmp(b_trim) {
                            Ordering::Equal => match a_num.len().cmp(&b_num.len()) {
                                Ordering::Equal => continue,
                                ord => return ord,
                            },
                            ord => return ord,
                        },
                        ord => return ord,
                    }
                } else {
                    let mut a_chunk = String::new();
                    while let Some(&c) = a_chars.peek() {
                        if !c.is_ascii_digit() { a_chunk.push(c); a_chars.next(); } else { break; }
                    }
                    let mut b_chunk = String::new();
                    while let Some(&c) = b_chars.peek() {
                        if !c.is_ascii_digit() { b_chunk.push(c); b_chars.next(); } else { break; }
                    }
                    let ord = a_chunk.to_ascii_lowercase().cmp(&b_chunk.to_ascii_lowercase());
                    if ord != Ordering::Equal { return ord; }
                    let ord2 = a_chunk.cmp(&b_chunk);
                    if ord2 != Ordering::Equal { return ord2; }
                }
            }
        }
    }
}
