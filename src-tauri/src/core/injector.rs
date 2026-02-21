use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum InjectionMessage {
    BeginSession,
    Delta(String),
    CommitSession,
    CancelSession,
    UndoLast,
}

pub fn spawn_injection_worker(mut rx: mpsc::Receiver<InjectionMessage>) {
    std::thread::spawn(move || {
        let mut enigo = Enigo::new(&Settings::default()).ok();
        let mut active_session = String::new();
        let mut last_session = String::new();

        while let Some(message) = rx.blocking_recv() {
            match message {
                InjectionMessage::BeginSession => {
                    active_session.clear();
                }
                InjectionMessage::Delta(delta) => {
                    if delta.is_empty() || secure_input_enabled() {
                        continue;
                    }
                    if let Some(enigo) = enigo.as_mut() {
                        for ch in delta.chars() {
                            let _ = enigo.text(&ch.to_string());
                            active_session.push(ch);
                        }
                    }
                }
                InjectionMessage::CommitSession => {
                    last_session = active_session.clone();
                    active_session.clear();
                }
                InjectionMessage::CancelSession => {
                    if let Some(enigo) = enigo.as_mut() {
                        backspace_text(enigo, active_session.chars().count());
                    }
                    active_session.clear();
                }
                InjectionMessage::UndoLast => {
                    if last_session.is_empty() {
                        continue;
                    }
                    if let Some(enigo) = enigo.as_mut() {
                        backspace_text(enigo, last_session.chars().count());
                    }
                    last_session.clear();
                }
            }
        }
    });
}

fn backspace_text(enigo: &mut Enigo, count: usize) {
    for _ in 0..count {
        let _ = enigo.key(Key::Backspace, Direction::Click);
    }
}

#[cfg(target_os = "macos")]
fn secure_input_enabled() -> bool {
    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn IsSecureEventInputEnabled() -> bool;
    }

    unsafe { IsSecureEventInputEnabled() }
}

#[cfg(not(target_os = "macos"))]
fn secure_input_enabled() -> bool {
    false
}
