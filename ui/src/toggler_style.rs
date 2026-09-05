//! Shared toggler (switch) style used by settings and Manage Models.
//! The default iced palette makes the off-knob ≈ off-track (both dark);
//! this style keeps the knob near-white on a muted dark track so the circle
//! is always visible.

use iced::{Color, Theme};
use iced::widget::toggler;

const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);

pub fn style(_theme: &Theme, status: toggler::Status) -> toggler::Style {
    use toggler::Status;
    let is_on = match status {
        Status::Active { is_toggled } | Status::Hovered { is_toggled } | Status::Disabled { is_toggled } => {
            is_toggled
        }
    };
    let is_hovered = matches!(status, Status::Hovered { .. });
    let is_disabled = matches!(status, Status::Disabled { .. });
    if is_disabled {
        return toggler::Style {
            background: Color::from_rgba8(55, 55, 65, 0.6).into(),
            background_border_width: 0.0,
            background_border_color: Color::TRANSPARENT,
            foreground: Color::from_rgb8(140, 140, 150).into(),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: Some(MUTED_FG),
            border_radius: None,
            padding_ratio: 0.12,
        };
    }
    if is_on {
        toggler::Style {
            background: (if is_hovered {
                crate::accent::accent_hover()
            } else {
                crate::accent::accent()
            })
            .into(),
            background_border_width: 0.0,
            background_border_color: Color::TRANSPARENT,
            foreground: Color::WHITE.into(),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: Some(Color::WHITE),
            border_radius: None,
            padding_ratio: 0.12,
        }
    } else {
        toggler::Style {
            background: (if is_hovered {
                Color::from_rgb8(78, 78, 88)
            } else {
                Color::from_rgb8(62, 62, 72)
            })
            .into(),
            background_border_width: 1.0,
            background_border_color: Color::from_rgba8(255, 255, 255, 0.08),
            foreground: Color::from_rgb8(232, 232, 236).into(),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: Some(MUTED_FG),
            border_radius: None,
            padding_ratio: 0.12,
        }
    }
}
