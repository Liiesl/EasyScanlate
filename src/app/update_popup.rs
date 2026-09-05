// Thin Message wrapper — widget construction lives in `easyscanlate-ui::dialog::update`.
use iced::widget::{center, column, container, opaque, space, stack};
use iced::{Color, Element, Length};
use super::{App, Message};

/// Blocking update-available popup over `base` with dim + blurred card
/// backdrop. Returns `base` unchanged when the popup should stay hidden
/// (not visible, nothing to offer, onboarding blocking, loading/exporting,
/// another modal open, or a close confirmation pending).
pub fn view<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    if !should_show(app) {
        return base;
    }
    let Some(window) = easyscanlate_ui::dialog::update::window(app) else {
        return base;
    };
    let window_mapped = window.map(Message::from);
    let h = app.frame.config().title_bar_height;
    let title_dim = container(space::horizontal().width(Length::Fill).height(Length::Fixed(h)))
        .width(Length::Fill)
        .height(Length::Fixed(h))
        .style(|_| container::Style {
            background: Some(Color { a: 0.45, ..Color::BLACK }.into()),
            ..container::Style::default()
        });
    // Blocking: opaque with no backdrop-click dismiss (use Later button).
    let content_dim = opaque(
        container(center(opaque(window_mapped)))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color { a: 0.45, ..Color::BLACK }.into()),
                ..container::Style::default()
            })
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    );
    stack![
        base,
        column![title_dim, content_dim]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}

/// Mirrors the layering guards in `src/app/view.rs`: the popup never covers
/// onboarding, the loading/export splashes, other modals, or the close
/// confirmation (those win).
fn should_show(app: &App) -> bool {
    if !app.update_popup_visible || app.update_info.is_none() {
        return false;
    }
    if app.onboarding.is_some() || app.pending_close.is_some() {
        return false;
    }
    if app.settings_open
        || app.manage_models_open
        || app.connect_modal.is_some()
        || app.new_project.is_some()
    {
        return false;
    }
    let is_loading =
        !app.active_is_home() && app.tabs.get(app.active).is_some_and(|t| t.loading);
    if is_loading {
        return false;
    }
    let is_exporting =
        !is_loading && !app.active_is_home() && app.tabs.get(app.active).is_some_and(|t| t.exporting);
    if is_exporting {
        return false;
    }
    true
}
