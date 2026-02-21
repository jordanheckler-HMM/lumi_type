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
                    let mut reset_enigo = false;
                    if let Some(writer) = ensure_enigo(&mut enigo) {
                        for ch in delta.chars() {
                            if writer.text(&ch.to_string()).is_err() {
                                reset_enigo = true;
                                break;
                            }
                            active_session.push(ch);
                        }
                    }
                    if reset_enigo {
                        enigo = None;
                    }
                }
                InjectionMessage::CommitSession => {
                    last_session = active_session.clone();
                    active_session.clear();
                }
                InjectionMessage::CancelSession => {
                    let mut reset_enigo = false;
                    if let Some(writer) = ensure_enigo(&mut enigo) {
                        if backspace_text(writer, active_session.chars().count()).is_err() {
                            reset_enigo = true;
                        }
                    }
                    if reset_enigo {
                        enigo = None;
                    }
                    active_session.clear();
                }
                InjectionMessage::UndoLast => {
                    if last_session.is_empty() {
                        continue;
                    }
                    let mut reset_enigo = false;
                    if let Some(writer) = ensure_enigo(&mut enigo) {
                        if backspace_text(writer, last_session.chars().count()).is_err() {
                            reset_enigo = true;
                        }
                    }
                    if reset_enigo {
                        enigo = None;
                    }
                    last_session.clear();
                }
            }
        }
    });
}

fn ensure_enigo(enigo: &mut Option<Enigo>) -> Option<&mut Enigo> {
    if enigo.is_none() {
        *enigo = Enigo::new(&Settings::default()).ok();
    }
    enigo.as_mut()
}

fn backspace_text(enigo: &mut Enigo, count: usize) -> Result<(), ()> {
    for _ in 0..count {
        enigo.key(Key::Backspace, Direction::Click).map_err(|_| ())?;
    }
    Ok(())
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
