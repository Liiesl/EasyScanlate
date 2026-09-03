// Thin Message wrapper — widget construction lives in `easyscanlate-ui::dialog::confirm_close`.
use iced::widget::{center, column, container, mouse_area, opaque, space, stack};
use iced::{Color, Element, Length};
use super::{App, Message};

pub fn view<'a>(app: &'a App, base: Element<'a, Message>) -> Element<'a, Message> {
    let Some(window) = easyscanlate_ui::dialog::confirm_close::window(app) else {
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
    let content_dim = opaque(mouse_area(
        container(center(opaque(window_mapped)))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(Color { a: 0.45, ..Color::BLACK }.into()),
                ..container::Style::default()
            })
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .on_press(Message::Ui(easyscanlate_ui::event::UiEvent::TabCloseCancel)));
    stack![
        base,
        column![title_dim, content_dim]
            .width(Length::Fill)
            .height(Length::Fill)
    ]
    .into()
}
