//! Shared embedded scrollbars (no content overlap).
//!
//! Floating (default) scrollbars paint over content. Every `scrollable` in
//! this crate should use the embedded API so the bar takes layout space:
//! either `scrollable(content).spacing(scale::s(EMBEDDED_SPACING))` or an
//! explicit `Direction::{Vertical,Horizontal}` built from
//! [`vertical()`] / [`horizontal()`].

/// Design-time gap between content and an embedded scrollbar, in px at 12pt.
/// Always scale via `scale::s()` at the call site.
pub const EMBEDDED_SPACING: f32 = 8.0;

/// Embedded vertical scrollbar with the shared spacing.
pub fn vertical() -> iced::widget::scrollable::Scrollbar {
    iced::widget::scrollable::Scrollbar::new().spacing(crate::scale::s(EMBEDDED_SPACING))
}

/// Embedded horizontal scrollbar with the shared spacing.
pub fn horizontal() -> iced::widget::scrollable::Scrollbar {
    iced::widget::scrollable::Scrollbar::new().spacing(crate::scale::s(EMBEDDED_SPACING))
}
