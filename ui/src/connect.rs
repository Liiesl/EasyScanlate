//! The "connect translation service" modal: a small overlay opened from the
//! settings modal when the user presses Connect on a provider (or on the
//! custom OpenAI-/Anthropic-compatible slots). It collects the API key and,
//! for custom endpoints, the base URL and the model id, then the app stores
//! the connection on submit.

use iced::widget::{
    button, center, column, container, mouse_area, opaque, row, space, stack, text, text_input,
};
use iced::{Color, Element, Fill as FillLength, Length};

use crate::event::UiEvent;
use crate::panel::PANEL_BG;
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
    let title = scanlateit_translation::provider_name(&modal.provider_id);

    let mut fields: Vec<Element<'_, UiEvent>> = Vec::new();
    fields.push(text("API key").size(12).color(MUTED_FG).into());
    fields.push(
        text_input("sk-…", &modal.api_key)
            .on_input(UiEvent::ConnectModalKey)
            .secure(true)
            .padding(4)
            .size(12)
            .width(FillLength)
            .into(),
    );
    if modal.is_custom {
        fields.push(text("Base URL").size(12).color(MUTED_FG).into());
        fields.push(
            text_input(
                "http://localhost:11434/v1",
                &modal.base_url,
            )
            .on_input(UiEvent::ConnectModalBaseUrl)
            .padding(4)
            .size(12)
            .width(FillLength)
            .into(),
        );
        fields.push(text("Model").size(12).color(MUTED_FG).into());
        fields.push(
            text_input("llama-3.1-8b", &modal.model)
                .on_input(UiEvent::ConnectModalModel)
                .padding(4)
                .size(12)
                .width(FillLength)
                .into(),
        );
    }
    if let Some(error) = &modal.error {
        fields.push(text(error).size(11).color(ERROR_FG).into());
    }

    let window = container(
        column![
            row![
                text(title).size(16),
                space::horizontal(),
                button(text("✕"))
                    .padding(2)
                    .on_press(UiEvent::ConnectModalCancel),
            ],
            column(fields).spacing(4).height(FillLength),
            row![
                space::horizontal(),
                button(text("Cancel"))
                    .padding([4, 10])
                    .on_press(UiEvent::ConnectModalCancel),
                button(text("Connect"))
                    .padding([4, 10])
                    .on_press(UiEvent::ConnectModalSubmit),
            ]
            .spacing(6),
        ]
        .spacing(10),
    )
    .width(Length::Fixed(MODAL_WIDTH))
    .height(Length::Fixed(MODAL_HEIGHT))
    .padding(12)
    .style(|_theme| container::Style {
        background: Some(PANEL_BG.into()),
        border: iced::Border::default().rounded(8).color(Color::from_rgb8(60, 63, 74)).width(1),
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
