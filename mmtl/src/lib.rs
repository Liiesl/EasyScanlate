//! .mmtl persistence for Scanlateit.
//! Single `project.xml` inside a ZIP, plus `images/` and optional `inpaint/`.
//! Legacy ManhwaOCR JSON import/export also lives here.

pub mod legacy;
pub mod xml;
pub mod zip;

pub use xml::{from_xml_str, to_xml_string};
pub use zip::{load_mmtl, save_mmtl, InpaintImageData, LoadResult};
