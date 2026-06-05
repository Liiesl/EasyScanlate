//! Pure data model: the app's single source of truth.
//!
//! Rules enforced here:
//! - [`OcrResult`] (OCR text) is append-only; deletion is a soft-delete flag.
//! - [`Profiles`] are a freely editable delta layer on top of it; one profile
//!   is selected at a time and the whole UI reads through it.
//! - Entry styles are a per-OCR-result dict shared by every profile (see
//!   [`Project`]).
//! - [`Extras`] (notes, inpainting, geometries) survives across profiles.
//!
//! This layer depends only on `std`: no iced, no OCR engine, no I/O.

pub mod entry;
pub mod extras;
pub mod ocr_result;
pub mod profile;
pub mod project;
pub mod style;

pub use entry::{EntryId, EntrySource, NewEntry, OcrEntry, Quad};
pub use extras::{Extras, InpaintPatch};
pub use ocr_result::OcrResult;
pub use profile::{ProfileId, Profiles};
pub use project::Project;
pub use style::EntryStyle;