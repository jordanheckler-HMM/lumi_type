use serde::Serialize;

use super::{permissions::PermissionStatus, AudioFrame, TranscriptionModel};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum DictationState {
    Idle,
    Listening,
    Dictating,
    Stopping,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    Listening,
    Dictating,
}

#[derive(Debug, Clone)]
pub enum EngineCommand {
    AudioFrame(AudioFrame),
    WakeDetected,
    PushToTalkTriggered,
    SilenceTimeout,
    TranscriptionDelta(String),
    TranscriptionFinished,
    CancelDictation,
    UndoLastDictation,
    SetEnabled(bool),
    UpdateSensitivity(f32),
    UpdateModel(TranscriptionModel),
    PermissionsChecked(PermissionStatus),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "payload")]
pub enum EngineEvent {
    StateChanged(DictationState),
    TrayStateChanged(TrayState),
    OverlayVisibility(bool),
    OverlayReset,
    OverlayTextDelta(String),
    OverlayWave(f32),
    PermissionsRequired(PermissionStatus),
    Error(String),
}

#[derive(Debug)]
pub struct StateMachine {
    state: DictationState,
    enabled: bool,
}

impl StateMachine {
    pub fn new(enabled: bool) -> Self {
        let state = if enabled {
            DictationState::Listening
        } else {
            DictationState::Idle
        };
        Self { state, enabled }
    }

    pub fn state(&self) -> DictationState {
        self.state
    }

    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        self.enabled = enabled;
        let next = if enabled {
            DictationState::Listening
        } else {
            DictationState::Idle
        };
        self.transition_to(next)
    }

    pub fn try_start_dictation(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        self.transition_to(DictationState::Dictating)
    }

    pub fn try_begin_stopping(&mut self) -> bool {
        if self.state != DictationState::Dictating {
            return false;
        }
        self.transition_to(DictationState::Stopping)
    }

    pub fn finish_stopping(&mut self) -> bool {
        if !matches!(self.state, DictationState::Stopping | DictationState::Dictating) {
            return false;
        }
        let next = if self.enabled {
            DictationState::Listening
        } else {
            DictationState::Idle
        };
        self.transition_to(next)
    }

    pub fn cancel_dictation(&mut self) -> bool {
        if !matches!(self.state, DictationState::Dictating | DictationState::Stopping) {
            return false;
        }
        let next = if self.enabled {
            DictationState::Listening
        } else {
            DictationState::Idle
        };
        self.transition_to(next)
    }

    pub fn should_route_to_wake(&self) -> bool {
        self.enabled && self.state == DictationState::Listening
    }

    pub fn should_route_to_dictation(&self) -> bool {
        self.enabled && self.state == DictationState::Dictating
    }

    pub fn tray_state(&self) -> TrayState {
        match self.state {
            DictationState::Idle => TrayState::Idle,
            DictationState::Listening => TrayState::Listening,
            DictationState::Dictating | DictationState::Stopping => TrayState::Dictating,
        }
    }

    fn transition_to(&mut self, next: DictationState) -> bool {
        if self.state == next {
            return false;
        }
        self.state = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{DictationState, StateMachine};

    #[test]
    fn starts_listening_when_enabled() {
        let machine = StateMachine::new(true);
        assert_eq!(machine.state(), DictationState::Listening);
    }

    #[test]
    fn starts_idle_when_disabled() {
        let machine = StateMachine::new(false);
        assert_eq!(machine.state(), DictationState::Idle);
    }

    #[test]
    fn dictation_flow_transitions_are_valid() {
        let mut machine = StateMachine::new(true);
        assert!(machine.try_start_dictation());
        assert_eq!(machine.state(), DictationState::Dictating);

        assert!(machine.try_begin_stopping());
        assert_eq!(machine.state(), DictationState::Stopping);

        assert!(machine.finish_stopping());
        assert_eq!(machine.state(), DictationState::Listening);
    }

    #[test]
    fn cancel_returns_to_listening() {
        let mut machine = StateMachine::new(true);
        assert!(machine.try_start_dictation());
        assert!(machine.cancel_dictation());
        assert_eq!(machine.state(), DictationState::Listening);
    }

    #[test]
    fn ten_consecutive_dictations_rearm_without_invalid_state() {
        let mut machine = StateMachine::new(true);

        for _ in 0..10 {
            assert_eq!(machine.state(), DictationState::Listening);
            assert!(machine.try_start_dictation());
            assert_eq!(machine.state(), DictationState::Dictating);
            assert!(machine.try_begin_stopping());
            assert_eq!(machine.state(), DictationState::Stopping);
            assert!(machine.finish_stopping());
            assert_eq!(machine.state(), DictationState::Listening);
        }
    }
}
