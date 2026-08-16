//! The "connect translation service" modal: a small overlay opened from the
//! settings modal when the user presses Connect on a provider (or on the
//! custom OpenAI-/Anthropic-compatible slots). It collects the API key and,
//! for custom endpoints, the base URL and the model id, then the app stores
//! the connection on submit.

use iced::widget::{
    button, center, column, container, mouse_area, opaque, row, space, stack, text, text_input, tooltip,
};
use iced::{Color, Element, Fill as FillLength, Length};
use lucide_icons::Icon;

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
use crate::scale;
use crate::state::UiState;

const MODAL_WIDTH: f32 = 360.0;
const MODAL_HEIGHT: f32 = 250.0;
const ACCENT: Color = Color::from_rgb8(92, 190, 255);
const MUTED_FG: Color = Color::from_rgb(0.6, 0.6, 0.6);
const ERROR_FG: Color = Color::from_rgb8(255, 110, 110);

/// The in-memory state of the connect modal: which provider it configures
/// and the values typed into its fields. Owned by the app, rendered here.
#[derive(Debug, Clone)]
pub struct ConnectModal {
    /// The connection id being configured (`openai`, `deepseek`,
    /// `custom-openai`, ...).
    pub provider_id: String,
    /// True for the custom endpoints, which also collect a base URL and a
    /// model id.
    pub is_custom: bool,
    /// The API key typed so far.
    pub api_key: String,
    /// The base URL typed so far (custom endpoints only).
    pub base_url: String,
    /// The model id typed so far (custom endpoints only).
    pub model: String,
    /// Validation/connection error to show under the fields.
    pub error: Option<String>,
}

/// The connect overlay: `base` (the settings modal underneath, already
/// dimmed) under a second, smaller centered window. Clicking the backdrop
/// cancels the modal.
pub fn view<'a, S: UiState + ?Sized>(
    state: &'a S,
    base: Element<'a, UiEvent>,
) -> Element<'a, UiEvent> {
    let modal = state
        .connect_modal()
        .expect("connect modal view is only rendered while the modal is open");
    let title = crate::translation::provider_name(&modal.provider_id);

    let is_local = crate::translation::is_local(&modal.provider_id);
    let mut fields: Vec<Element<'_, UiEvent>> = Vec::new();
    if !is_local {
        fields.push(text("API key").size(scale::s(12.0)).color(MUTED_FG).into());
        fields.push(
            text_input("sk-…", &modal.api_key)
                .on_input(UiEvent::ConnectModalKey)
                .secure(true)
                .padding(scale::s(4.0))
                .size(scale::s(12.0))
                .width(FillLength)
                .into(),
        );
    }
    if modal.is_custom || is_local {
        let placeholder = if is_local {
            match modal.provider_id.as_str() {
                crate::translation::LOCAL_OLLAMA => "http://localhost:11434",
                crate::translation::LOCAL_VLLM => "http://localhost:8000/v1",
                crate::translation::LOCAL_LLAMA_CPP => "http://localhost:8080/v1",
                _ => "http://localhost:11434",
            }
        } else {
            "http://localhost:11434/v1"
        };
        fields.push(text("Base URL").size(scale::s(12.0)).color(MUTED_FG).into());
        fields.push(
            text_input(placeholder, &modal.base_url)
                .on_input(UiEvent::ConnectModalBaseUrl)
                .padding(scale::s(4.0))
                .size(scale::s(12.0))
                .width(FillLength)
                .into(),
        );
        if is_local {
            fields.push(
                text("Models are discovered automatically from the endpoint.")
                    .size(scale::s(11.0))
                    .color(MUTED_FG)
                    .into(),
            );
        }
    }
    if modal.is_custom {
        fields.push(text("Model").size(scale::s(12.0)).color(MUTED_FG).into());
        fields.push(
            text_input("llama-3.1-8b", &modal.model)
                .on_input(UiEvent::ConnectModalModel)
                .padding(scale::s(4.0))
                .size(scale::s(12.0))
                .width(FillLength)
                .into(),
        );
    }
    if let Some(error) = &modal.error {
        fields.push(text(error).size(scale::s(11.0)).color(ERROR_FG).into());
    }

    let window = container(
        column![
            row![
                text(title).size(scale::s(16.0)),
                space::horizontal(),
                tooltip(
                    button(crate::icon::lucide(Icon::X).size(scale::s(14.0)).center())
                        .padding(scale::s(4.0))
                        .style(crate::panel::button_style)
            .on_press(UiEvent::ConnectModalCancel),
                    container(text("Close").size(scale::s(11.0))).padding(scale::s(6.0)).style(container::rounded_box),
                    tooltip::Position::Top
                ).gap(scale::s(4.0)),
            ],
            column(fields).spacing(scale::s(4.0)).height(FillLength),
            row![
                space::horizontal(),
                tooltip(
                    button(crate::icon::lucide(Icon::X).size(scale::s(14.0)).center())
                        .padding(scale::s(6.0))
                        .style(crate::panel::button_style)
            .on_press(UiEvent::ConnectModalCancel),
                    container(text("Cancel").size(scale::s(11.0))).padding(scale::s(6.0)).style(container::rounded_box),
                    tooltip::Position::Top
                ).gap(scale::s(4.0)),
                tooltip(
                    button(crate::icon::lucide(Icon::Plug).size(scale::s(14.0)).center())
                        .padding(scale::s(6.0))
                        .style(crate::panel::button_style)
            .on_press(UiEvent::ConnectModalSubmit),
                    container(text("Connect").size(scale::s(11.0))).padding(scale::s(6.0)).style(container::rounded_box),
                    tooltip::Position::Top
                ).gap(scale::s(4.0)),
            ]
            .spacing(scale::s(6.0)),
        ]
        .spacing(scale::s(10.0)),
    )
    .width(Length::Fixed(scale::s(MODAL_WIDTH)))
    .height(Length::Fixed(scale::s(MODAL_HEIGHT)))
    .padding(scale::s(12.0))
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(scale::s(8.0)).color(Color::from_rgb8(60, 63, 74)).width(scale::s(1.0)),
        ..container::Style::default()
    });

    stack![
        base,
        opaque(
            mouse_area(
                center(opaque(window)).style(|_theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.4,
                            ..Color::BLACK
                        }
                        .into()
                    ),
                    ..container::Style::default()
                })
            )
            .on_press(UiEvent::ConnectModalCancel)
        )
    ]
    .into()
}

/// The accent color used by connect buttons in the settings list.
pub const CONNECT_ACCENT: Color = ACCENT;
