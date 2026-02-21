use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use webrtc_vad::{SampleRate, Vad, VadMode};

use super::{state::EngineCommand, AudioFrame};

#[derive(Debug)]
pub enum VadMessage {
    Begin,
    Audio(AudioFrame),
    End,
    SetSensitivity(f32),
}

pub fn spawn_vad_worker(
    mut rx: mpsc::Receiver<VadMessage>,
    command_tx: mpsc::Sender<EngineCommand>,
    initial_sensitivity: f32,
) {
    std::thread::spawn(move || {
        let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);
        let mut silence_started: Option<Instant> = None;
        let mut sensitivity = initial_sensitivity.clamp(0.01, 1.0);
        let silence_timeout = Duration::from_secs_f32(1.0);

        while let Some(message) = rx.blocking_recv() {
            match message {
                VadMessage::Begin => {
                    vad.reset();
                    silence_started = None;
                }
                VadMessage::End => {
                    silence_started = None;
                }
                VadMessage::SetSensitivity(next) => {
                    sensitivity = next.clamp(0.01, 1.0);
                }
                VadMessage::Audio(frame) => {
                    let resampled = resample_mono_to_16k(&frame.samples, frame.sample_rate);
                    let energy_threshold = energy_threshold_from_sensitivity(sensitivity);
                    for chunk in resampled.chunks(320) {
                        if chunk.len() != 320 {
                            continue;
                        }

                        let vad_speech = vad.is_voice_segment(chunk).unwrap_or(false);
                        let energy_speech = chunk
                            .iter()
                            .map(|sample| (*sample as f32).abs() / i16::MAX as f32)
                            .sum::<f32>()
                            / chunk.len() as f32
                            > energy_threshold;

                        if vad_speech || energy_speech {
                            silence_started = None;
                            continue;
                        }

                        if let Some(started) = silence_started {
                            if started.elapsed() >= silence_timeout {
                                let _ = command_tx.blocking_send(EngineCommand::SilenceTimeout);
                                silence_started = None;
                                break;
                            }
                        } else {
                            silence_started = Some(Instant::now());
                        }
                    }
                }
            }
        }
    });
}

fn energy_threshold_from_sensitivity(sensitivity: f32) -> f32 {
    // Keep this threshold in a realistic speech-energy range.
    // Higher sensitivity should require less energy to classify as speech.
    let clamped = sensitivity.clamp(0.01, 1.0);
    0.12 - clamped * 0.10
}

pub fn resample_mono_to_16k(samples: &[i16], source_rate: u32) -> Vec<i16> {
    if source_rate == 16_000 {
        return samples.to_vec();
    }

    if samples.is_empty() || source_rate == 0 {
        return Vec::new();
    }

    let ratio = 16_000.0f32 / source_rate as f32;
    let target_len = ((samples.len() as f32) * ratio).max(1.0) as usize;
    let mut output = Vec::with_capacity(target_len);

    for idx in 0..target_len {
        let source_pos = (idx as f32) / ratio;
        let source_idx = source_pos.floor() as usize;
        let next_idx = (source_idx + 1).min(samples.len() - 1);
        let frac = source_pos - source_idx as f32;
        let current = samples[source_idx] as f32;
        let next = samples[next_idx] as f32;
        let interpolated = current + (next - current) * frac;
        output.push(interpolated.round() as i16);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::{energy_threshold_from_sensitivity, resample_mono_to_16k};

    #[test]
    fn resample_keeps_identity_at_16k() {
        let input = vec![1i16, 2, 3, 4];
        assert_eq!(resample_mono_to_16k(&input, 16_000), input);
    }

    #[test]
    fn resample_changes_length_when_rate_differs() {
        let input = vec![0i16; 48_000 / 10];
        let output = resample_mono_to_16k(&input, 48_000);
        assert!((output.len() as i32 - 1_600).abs() < 10);
    }

    #[test]
    fn sensitivity_maps_to_lower_energy_threshold_when_higher() {
        let low = energy_threshold_from_sensitivity(0.1);
        let high = energy_threshold_from_sensitivity(0.9);
        assert!(high < low);
    }
}
