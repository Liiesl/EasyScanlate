//! Machine translation via aisdk. Only the Opencode provider is wired up for
//! now; the rest of the app only ever sees the functions in this module.
//!
//! All OCR lines of all loaded images are translated in a single request.
//! The result is a `Vec<String>` aligned with the input order, which the app
//! stores into the selected profile named `english(auto)` style.

use aisdk::core::{DynamicModel, LanguageModelRequest};
use aisdk::providers::Opencode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Hard cap on lines per request; a single unbounded prompt is guaranteed to
/// blow the model's context window on big projects.
const MAX_LINES: usize = 1000;

/// Pre-configured model choices shown in the UI.
pub const MODELS: [&str; 3] = ["big-pickle", "mimo-v2.5-free", "deepseek-v4-flash-free"];

/// Target languages offered in the UI.
pub const LANGUAGES: [&str; 13] = [
    "English",
    "Korean",
    "Japanese",
    "Chinese (Simplified)",
    "Chinese (Traditional)",
    "Spanish",
    "French",
    "German",
    "Italian",
    "Portuguese",
    "Russian",
    "Thai",
    "Vietnamese",
];

/// Profile name convention for machine translations: `english(auto)`.
pub fn profile_name(lang: &str) -> String {
    format!("{}(auto)", lang.to_lowercase())
}

const SYSTEM: &str = "You are a professional scanlation translator for comics, manga and manhwa. \
Translate every line into the requested target language; detect the source language of each line \
yourself (most lines are Korean). Preserve meaning, tone, and the exact order and number of lines. \
Do not add, merge, drop or summarize any line. Do not add commentary, explanations, notes or any \
formatting such as markdown or code blocks. Output only the translations.";

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct TranslationBatch {
    translations: Vec<String>,
}

/// Translates every line in `texts` into `target` using the given Opencode
/// `model`. `api_key` overrides the `OPENCODE_API_KEY` environment variable
/// when set (in-memory only; never persisted). On success returns one
/// translation per input line, in the same order.
pub async fn translate_all(
    texts: &[String],
    target: &str,
    model: &str,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    if texts.len() > MAX_LINES {
        return Err(format!(
            "Too many lines for a single translation request ({}, max {MAX_LINES}).",
            texts.len()
        ));
    }

    let builder = Opencode::<DynamicModel>::builder().model_name(model);
    let builder = match api_key {
        Some(key) if !key.is_empty() => builder.api_key(key),
        _ => builder,
    };
    let provider = builder
        .build()
        .map_err(|e| format!("Translation init failed: {e} (set OPENCODE_API_KEY or enter an API key)"))?;

    let prompt = build_prompt(texts, target);
    eprintln!(
        "[translation] request: model={model} target={target} lines={}\nprompt:\n{prompt}\n---",
        texts.len()
    );

    match structured(provider.clone(), &prompt, texts.len()).await {
        Ok(translations) => {
            eprintln!("[translation] structured OK ({} lines)", translations.len());
            return Ok(translations);
        }
        Err(e) => eprintln!("[translation] structured attempt failed: {e}"),
    }

    let result = plain(provider, &prompt, texts.len()).await;
    eprintln!(
        "[translation] plain attempt: {}",
        match &result {
            Ok(lines) => format!("OK ({} lines)", lines.len()),
            Err(e) => format!("failed: {e}"),
        }
    );
    result
}

fn build_prompt(texts: &[String], target: &str) -> String {
    let mut prompt = format!(
        "Translate the following {n} line(s) into {target}.\n\nReturn a JSON object with a \
\"translations\" array containing exactly {n} strings in the same order, one per line.\n\n",
        n = texts.len(),
    );
    for (i, text) in texts.iter().enumerate() {
        let clean = text.replace(['\r', '\n'], " ");
        prompt.push_str(&format!("{}. {clean}\n", i + 1));
    }
    prompt
}

/// Attempt 1: structured JSON output via the provider's schema support.
async fn structured(
    provider: Opencode<DynamicModel>,
    prompt: &str,
    expected: usize,
) -> Result<Vec<String>, String> {
    let mut request = LanguageModelRequest::builder()
        .model(provider)
        .system(SYSTEM)
        .prompt(prompt)
        .schema::<TranslationBatch>()
        .temperature(10u32)
        .build();
    let response = request.generate_text().await.map_err(|e| e.to_string())?;
    eprintln!("[translation] structured response ({}):\n{}\n---", expected, response.text().unwrap_or_default());
    let batch: TranslationBatch = response.into_schema().map_err(|e| format!("Bad JSON response: {e}"))?;
    validate_count(batch.translations, expected)
}

/// Attempt 2: the same JSON prompt but without the `response_format` param,
/// which some endpoints (e.g. the opencode console gateway) reject. Models that
/// ignore the JSON instruction may still answer with numbered or plain lines,
/// which [`parse_response`] recovers.
async fn plain(
    provider: Opencode<DynamicModel>,
    prompt: &str,
    expected: usize,
) -> Result<Vec<String>, String> {
    let mut request = LanguageModelRequest::builder()
        .model(provider)
        .system(SYSTEM)
        .prompt(prompt)
        .temperature(10u32)
        .build();
    let response = request.generate_text().await.map_err(|e| e.to_string())?;
    let output = response.text().ok_or_else(|| "No text in model response.".to_string())?;
    eprintln!("[translation] plain response ({expected} expected):\n{output}\n---");
    let translations = parse_response(&output);
    validate_count(translations, expected)
}

/// Best-effort parsing of whatever the model actually output: a JSON object or
/// array first, then numbered lines, then plain lines as-is.
fn parse_response(output: &str) -> Vec<String> {
    parse_json_batch(output).unwrap_or_else(|| parse_numbered(output))
}

fn parse_json_batch(output: &str) -> Option<Vec<String>> {
    let trimmed = output.trim();
    let candidate = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim();
    if let Ok(batch) = serde_json::from_str::<TranslationBatch>(candidate) {
        return Some(batch.translations);
    }
    serde_json::from_str::<Vec<String>>(candidate).ok()
}

fn parse_numbered(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| {
            let rest = line.trim_start_matches(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9']);
            let rest = rest.trim_start_matches(['.', ')', ':', ' ', '"']);
            let rest = rest.trim_end_matches([' ', ',', '"']);
            if rest.is_empty() {
                line.to_string()
            } else {
                rest.to_string()
            }
        })
        .collect()
}

fn validate_count(lines: Vec<String>, expected: usize) -> Result<Vec<String>, String> {
    if lines.len() == expected {
        Ok(lines)
    } else {
        Err(format!(
            "Model returned {} translation(s) for {expected} line(s); skipping.",
            lines.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_name_is_lowercase_with_auto_suffix() {
        assert_eq!(profile_name("English"), "english(auto)");
        assert_eq!(profile_name("Chinese (Simplified)"), "chinese (simplified)(auto)");
    }

    #[test]
    fn numbered_lines_are_parsed_in_order() {
        let out = parse_numbered(
            "1. Hello\n2. How are you?\n\n3. Goodbye",
        );
        assert_eq!(out, vec!["Hello", "How are you?", "Goodbye"]);
    }

    #[test]
    fn unnumbered_output_is_kept_as_is() {
        let out = parse_numbered("Hello\nHow are you?\nGoodbye");
        assert_eq!(out, vec!["Hello", "How are you?", "Goodbye"]);
    }

    #[test]
    fn count_mismatch_is_rejected() {
        assert!(validate_count(vec!["a".to_string()], 2).is_err());
        assert_eq!(validate_count(vec!["a".to_string()], 1).unwrap(), vec!["a"]);
    }

    #[test]
    fn json_object_is_recovered_from_plain_response() {
        let out = parse_response(
            "{\n  \"translations\": [\n    \"Protect His Majesty.\",\n    \"Sukbin\",\n    \"You\"\n  ]\n}",
        );
        assert_eq!(out, vec!["Protect His Majesty.", "Sukbin", "You"]);
    }

    #[test]
    fn fenced_json_and_bare_arrays_are_recovered() {
        assert_eq!(
            parse_response("```json\n{\"translations\": [\"a\", \"b\"]}\n```"),
            vec!["a", "b"]
        );
        assert_eq!(parse_response("[\"a\", \"b\"]"), vec!["a", "b"]);
    }
}
