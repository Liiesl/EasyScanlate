//! .mmtl persistence for EasyScanlate.
//! Single `project.xml` inside a ZIP, plus `images/` and optional `inpaint/`.
//! Legacy ManhwaOCR JSON import/export also lives here.

pub mod legacy;
pub mod translation;
pub mod xml;
pub mod zip;

pub use translation::{TranslationLine, from_xml_str as translation_from_xml_str, to_xml_string as translation_to_xml_string};
pub use xml::{from_xml_str, to_xml_string};
pub use zip::{load_mmtl, save_mmtl, InpaintImageData, LoadResult};
