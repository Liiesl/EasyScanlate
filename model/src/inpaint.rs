use std::fmt;

use serde::{Deserialize, Serialize};

/// Which inpainting implementation the app uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InpaintBackend {
    /// The pure-Rust Telea algorithm from the `inpaint` crate: no model, no
    /// download, works instantly on CPU.
    #[default]
    Telea,
    /// The LaMa ONNX model: better on complex backgrounds, needs the
    /// `lama-manga.onnx` file next to the executable.
    Lama,
}

impl fmt::Display for InpaintBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Telea => "Telea (inpaint crate)",
            Self::Lama => "LaMa (ONNX)",
        })
    }
}