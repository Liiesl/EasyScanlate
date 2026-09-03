pub mod tabs;

use iced::widget::image::Handle as ImageHandle;
use std::sync::OnceLock;

pub fn title_icon_handle() -> Option<ImageHandle> {
    static ICON: OnceLock<Option<ImageHandle>> = OnceLock::new();
    ICON.get_or_init(|| {
        const BYTES: &[u8] = include_bytes!("../../../assets/app_icon.ico");
        match image::load_from_memory(BYTES) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width(), rgba.height());
                Some(ImageHandle::from_rgba(w, h, rgba.into_raw()))
            }
            Err(_) => None,
        }
    })
    .clone()
}
