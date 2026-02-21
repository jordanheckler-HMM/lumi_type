pub mod audio;
pub mod injector;
pub mod permissions;
pub mod state;
pub mod transcriber;
pub mod vad;
pub mod wake_word;

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use self::{
    injector::InjectionMessage,
    state::{DictationState, EngineCommand, EngineEvent, StateMachine},
    transcriber::TranscriberMessage,
    vad::VadMessage,
    wake_word::WakeWordConfig,
};

#[derive(Debug, Clone)]
pub struct AudioFrame {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub peak: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptionModel {
    BaseEn,
    TinyEn,
}

impl TranscriptionModel {
    pub fn file_name(self) -> &'static str {
        match self {
            TranscriptionModel::BaseEn => "ggml-base.en.bin",
            TranscriptionModel::TinyEn => "ggml-tiny.en.bin",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSettings {
    pub enabled: bool,
    pub launch_at_startup: bool,
    pub microphone: String,
    pub sensitivity: f32,
    pub model: TranscriptionModel,
    pub push_to_talk_hotkey: String,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            launch_at_startup: false,
            microphone: String::new(),
            sensitivity: 0.45,
            model: TranscriptionModel::BaseEn,
            push_to_talk_hotkey: "Cmd+Shift+Space".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct EngineHandle {
    command_tx: mpsc::Sender<EngineCommand>,
    events_tx: broadcast::Sender<EngineEvent>,
    settings: Arc<RwLock<EngineSettings>>,
}

impl EngineHandle {
    pub fn send_blocking(&self, command: EngineCommand) {
        let _ = self.command_tx.blocking_send(command);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events_tx.subscribe()
    }

    pub fn settings(&self) -> EngineSettings {
        self.settings.read().clone()
    }

    pub async fn apply_settings(&self, next: EngineSettings) {
        {
            *self.settings.write() = next.clone();
        }

        let _ = self
            .command_tx
            .send(EngineCommand::SetEnabled(next.enabled))
            .await;
        let _ = self
            .command_tx
            .send(EngineCommand::UpdateMicrophone(next.microphone.clone()))
            .await;
        let _ = self
            .command_tx
            .send(EngineCommand::UpdateSensitivity(next.sensitivity))
            .await;
        let _ = self
            .command_tx
            .send(EngineCommand::UpdateModel(next.model))
            .await;
    }
}

pub fn spawn_engine(initial_settings: EngineSettings, model_root: PathBuf) -> Result<EngineHandle> {
    let settings = Arc::new(RwLock::new(initial_settings.clone()));

    let (command_tx, mut command_rx) = mpsc::channel::<EngineCommand>(1024);
    let (events_tx, _) = broadcast::channel::<EngineEvent>(1024);

    let (wake_tx, wake_rx) = mpsc::channel::<AudioFrame>(128);
    let (vad_tx, vad_rx) = mpsc::channel::<VadMessage>(128);
    let (transcriber_tx, transcriber_rx) = mpsc::channel::<TranscriberMessage>(128);
    let (injector_tx, injector_rx) = mpsc::channel::<InjectionMessage>(128);

    let wake_config = WakeWordConfig::from_model_root(&model_root, initial_settings.sensitivity)
        .with_overrides_from_env();
    wake_word::spawn_wake_listener(wake_rx, command_tx.clone(), wake_config);
    vad::spawn_vad_worker(vad_rx, command_tx.clone(), initial_settings.sensitivity);
    transcriber::spawn_transcriber_worker(
        transcriber_rx,
        command_tx.clone(),
        model_root,
        initial_settings.model,
    );
    injector::spawn_injection_worker(injector_rx);

    let events_tx_for_loop = events_tx.clone();
    let command_tx_for_audio = command_tx.clone();
    std::thread::spawn(move || {
        let mut preferred_microphone = initial_settings.microphone.clone();
        let mut audio_capture = try_start_audio_capture(
            &command_tx_for_audio,
            preferred_microphone.as_str(),
            &events_tx_for_loop,
        );

        let mut machine = StateMachine::new(initial_settings.enabled);
        emit_state_events(&events_tx_for_loop, &machine);

        while let Some(command) = command_rx.blocking_recv() {
            match command {
                EngineCommand::AudioFrame(frame) => {
                    if machine.should_route_to_wake() {
                        let _ = wake_tx.try_send(frame.clone());
                    }

                    if machine.should_route_to_dictation() {
                        let _ = vad_tx.try_send(VadMessage::Audio(frame.clone()));
                        let _ = transcriber_tx.try_send(TranscriberMessage::Audio(frame.clone()));
                        let _ = events_tx_for_loop.send(EngineEvent::OverlayWave(frame.peak));
                    }
                }
                EngineCommand::WakeDetected | EngineCommand::PushToTalkTriggered => {
                    if machine.try_start_dictation() {
                        let _ = transcriber_tx.blocking_send(TranscriberMessage::Begin);
                        let _ = vad_tx.blocking_send(VadMessage::Begin);
                        let _ = injector_tx.blocking_send(InjectionMessage::BeginSession);

                        let _ = events_tx_for_loop.send(EngineEvent::OverlayReset);
                        let _ = events_tx_for_loop.send(EngineEvent::OverlayVisibility(true));
                        emit_state_events(&events_tx_for_loop, &machine);
                    }
                }
                EngineCommand::SilenceTimeout => {
                    if machine.try_begin_stopping() {
                        let _ = vad_tx.blocking_send(VadMessage::End);
                        let _ = transcriber_tx.blocking_send(TranscriberMessage::End);
                        emit_state_events(&events_tx_for_loop, &machine);
                    }
                }
                EngineCommand::TranscriptionDelta(delta) => {
                    if matches!(
                        machine.state(),
                        DictationState::Dictating | DictationState::Stopping
                    ) {
                        let _ =
                            events_tx_for_loop.send(EngineEvent::OverlayTextDelta(delta.clone()));
                        let _ = injector_tx.blocking_send(InjectionMessage::Delta(delta));
                    }
                }
                EngineCommand::TranscriptionFinished => {
                    let _ = injector_tx.blocking_send(InjectionMessage::CommitSession);
                    if machine.finish_stopping() {
                        let _ = events_tx_for_loop.send(EngineEvent::OverlayVisibility(false));
                        let _ = events_tx_for_loop.send(EngineEvent::OverlayReset);
                        emit_state_events(&events_tx_for_loop, &machine);
                    }
                }
                EngineCommand::CancelDictation => {
                    if machine.cancel_dictation() {
                        let _ = transcriber_tx.blocking_send(TranscriberMessage::Cancel);
                        let _ = vad_tx.blocking_send(VadMessage::End);
                        let _ = injector_tx.blocking_send(InjectionMessage::CancelSession);
                        let _ = events_tx_for_loop.send(EngineEvent::OverlayVisibility(false));
                        let _ = events_tx_for_loop.send(EngineEvent::OverlayReset);
                        emit_state_events(&events_tx_for_loop, &machine);
                    }
                }
                EngineCommand::UndoLastDictation => {
                    let _ = injector_tx.blocking_send(InjectionMessage::UndoLast);
                }
                EngineCommand::SetEnabled(enabled) => {
                    if machine.set_enabled(enabled) {
                        if !enabled {
                            let _ = transcriber_tx.blocking_send(TranscriberMessage::Cancel);
                            let _ = vad_tx.blocking_send(VadMessage::End);
                            let _ = injector_tx.blocking_send(InjectionMessage::CancelSession);
                            let _ = events_tx_for_loop.send(EngineEvent::OverlayVisibility(false));
                            let _ = events_tx_for_loop.send(EngineEvent::OverlayReset);
                        }
                        emit_state_events(&events_tx_for_loop, &machine);
                    }
                }
                EngineCommand::UpdateMicrophone(microphone) => {
                    preferred_microphone = microphone;
                    audio_capture = try_start_audio_capture(
                        &command_tx_for_audio,
                        preferred_microphone.as_str(),
                        &events_tx_for_loop,
                    );
                }
                EngineCommand::UpdateSensitivity(value) => {
                    let _ = vad_tx.blocking_send(VadMessage::SetSensitivity(value));
                }
                EngineCommand::UpdateModel(model) => {
                    let _ = transcriber_tx.blocking_send(TranscriberMessage::UpdateModel(model));
                }
                EngineCommand::PermissionsChecked(status) => {
                    if status.microphone && audio_capture.is_none() {
                        audio_capture = try_start_audio_capture(
                            &command_tx_for_audio,
                            preferred_microphone.as_str(),
                            &events_tx_for_loop,
                        );
                    }
                    if !status.all_granted() {
                        let _ = events_tx_for_loop.send(EngineEvent::PermissionsRequired(status));
                    }
                }
            }
        }
    });

    Ok(EngineHandle {
        command_tx,
        events_tx,
        settings,
    })
}

fn emit_state_events(events_tx: &broadcast::Sender<EngineEvent>, machine: &StateMachine) {
    let _ = events_tx.send(EngineEvent::StateChanged(machine.state()));
    let _ = events_tx.send(EngineEvent::TrayStateChanged(machine.tray_state()));
}

fn try_start_audio_capture(
    command_tx: &mpsc::Sender<EngineCommand>,
    preferred_microphone: &str,
    events_tx: &broadcast::Sender<EngineEvent>,
) -> Option<audio::AudioCapture> {
    let preferred = if preferred_microphone.trim().is_empty() {
        None
    } else {
        Some(preferred_microphone.to_string())
    };

    match audio::AudioCapture::start(command_tx.clone(), preferred) {
        Ok(capture) => Some(capture),
        Err(_) => {
            let _ = events_tx.send(EngineEvent::Error(
                "Unable to start microphone stream; check microphone permission and selected device."
                    .to_string(),
            ));
            None
        }
    }
}
