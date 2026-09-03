use std::time::Duration;

use iced::Subscription;

use super::{App, Message, TabMessage};
use easyscanlate_ui::event::UiEvent;

#[derive(Clone, Hash)]
struct KeysState {
    ids: Vec<u64>,
    active: u64,
    len: usize,
}

fn keys_subscription(app: &App) -> Subscription<Message> {
    let tab_ids: Vec<u64> = app.tabs.iter().map(|t| t.id.0).collect();
    let active_id = app.tabs.get(app.active).map(|t| t.id.0).unwrap_or(0);
    let active_len = app.tabs.len();
    let keys_state = KeysState { ids: tab_ids, active: active_id, len: active_len };
    iced::event::listen().with(keys_state).filter_map(|(state, event)| {
        if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
            if modifiers.control() && !modifiers.shift() {
                match key.as_ref() {
                    iced::keyboard::Key::Character(c) if c == "s" || c == "S" => return Some(Message::Ui(UiEvent::SaveProject)),
                    iced::keyboard::Key::Character(c) if c == "o" || c == "O" => return Some(Message::Ui(UiEvent::HomeOpenProject)),
                    iced::keyboard::Key::Character(c) if c == "t" || c == "T" => return Some(Message::Ui(UiEvent::TabNew)),
                    iced::keyboard::Key::Character(c) if c == "w" || c == "W" => return Some(Message::Ui(UiEvent::TabClose(state.active))),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) => {
                        if state.len > 1 {
                            let next_idx = (state.ids.iter().position(|&id| id == state.active).unwrap_or(0) + 1) % state.len;
                            return Some(Message::Ui(UiEvent::TabSelected(state.ids[next_idx])));
                        }
                        return None;
                    }
                    iced::keyboard::Key::Character(c) => {
                        if let Ok(n) = c.parse::<usize>() {
                            if (1..=9).contains(&n) && n <= state.len {
                                return Some(Message::Ui(UiEvent::TabSelected(state.ids[n - 1])));
                            }
                        }
                        return None;
                    }
                    _ => return None,
                }
            } else if modifiers.control() && modifiers.shift() {
                match key.as_ref() {
                    iced::keyboard::Key::Character(c) if c == "w" || c == "W" => return Some(Message::Ui(UiEvent::TabCloseAll)),
                    iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab) => {
                        if state.len > 1 {
                            let pos = state.ids.iter().position(|&id| id == state.active).unwrap_or(0);
                            let prev_idx = if pos == 0 { state.len - 1 } else { pos - 1 };
                            return Some(Message::Ui(UiEvent::TabSelected(state.ids[prev_idx])));
                        }
                        return None;
                    }
                    _ => return None,
                }
            }
            None
        } else {
            None
        }
    })
}

fn drops_subscription() -> Subscription<Message> {
    iced::event::listen().filter_map(|event| match event {
        iced::Event::Window(iced::window::Event::FileDropped(path)) => {
            let s = path.to_string_lossy().to_string();
            if s.to_ascii_lowercase().ends_with(".mmtl") {
                Some(Message::ExternalOpen(vec![s]))
            } else {
                None
            }
        }
        _ => None,
    })
}

fn ticks_subscriptions(app: &App) -> Vec<Subscription<Message>> {
    let mut subs = Vec::new();
    for tab in &app.tabs {
        let tid = tab.id;
        #[cfg(feature = "ocr")]
        if tab.running {
            subs.push(iced::time::every(Duration::from_millis(16)).with(tid).map(|(tid, _)| Message::Tab(tid, TabMessage::OcrTick)));
        }
        if tab.translating {
            subs.push(iced::time::every(Duration::from_millis(16)).with(tid).map(|(tid, _)| Message::Tab(tid, TabMessage::TranslateTick)));
        }
    }
    subs
}

pub fn subscription(app: &App) -> Subscription<Message> {
    let frame_sub = app.frame.subscription().map(Message::Frame);
    let mut subs = vec![frame_sub, keys_subscription(app), drops_subscription()];

    if app.ipc_listener.is_some() {
        subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Message::IpcPoll));
    }

    subs.extend(ticks_subscriptions(app));

    if app.update_downloading {
        subs.push(iced::time::every(Duration::from_millis(100)).map(|_| Message::UpdatePoll));
    }

    if app.onboarding.as_ref().is_some_and(|o| o.downloading) {
        subs.push(iced::time::every(Duration::from_millis(100)).map(|_| Message::OnboardingModelPoll));
    }

    Subscription::batch(subs)
}
