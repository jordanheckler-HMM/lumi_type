use std::{path::PathBuf, time::{Duration, Instant}};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use whisper_rs::{
    convert_integer_to_float_audio, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

use super::{
    state::EngineCommand,
    vad::resample_mono_to_16k,
    AudioFrame,
    TranscriptionModel,
};

#[derive(Debug)]
pub enum TranscriberMessage {
    Begin,
    Audio(AudioFrame),
    End,
    Cancel,
    UpdateModel(TranscriptionModel),
}

pub fn spawn_transcriber_worker(
    mut rx: mpsc::Receiver<TranscriberMessage>,
    command_tx: mpsc::Sender<EngineCommand>,
    model_root: PathBuf,
    initial_model: TranscriptionModel,
) {
    tauri::async_runtime::spawn(async move {
        let mut runtime = match TranscriberRuntime::new(model_root.clone(), initial_model) {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("transcriber disabled: {error}");
                return;
            }
        };

        let mut session_audio = Vec::<i16>::new();
        let mut last_emitted = String::new();
        let mut last_decode_at = Instant::now();

        while let Some(message) = rx.recv().await {
            match message {
                TranscriberMessage::Begin => {
                    session_audio.clear();
                    last_emitted.clear();
                    last_decode_at = Instant::now();
                }
                TranscriberMessage::Audio(frame) => {
                    session_audio.extend(resample_mono_to_16k(&frame.samples, frame.sample_rate));
                    if last_decode_at.elapsed() < Duration::from_millis(350) {
                        continue;
                    }
                    if session_audio.len() < 3200 {
                        continue;
                    }

                    if let Ok(text) = runtime.transcribe(&session_audio, false) {
                        let delta = transcript_delta(&last_emitted, &text);
                        if !delta.is_empty() {
                            let _ = command_tx
                                .send(EngineCommand::TranscriptionDelta(delta.clone()))
                                .await;
                        }
                        last_emitted = text;
                    }
                    last_decode_at = Instant::now();
                }
                TranscriberMessage::End => {
                    if let Ok(text) = runtime.transcribe(&session_audio, true) {
                        let delta = transcript_delta(&last_emitted, &text);
                        if !delta.is_empty() {
                            let _ = command_tx
                                .send(EngineCommand::TranscriptionDelta(delta))
                                .await;
                        }
                    }
                    session_audio.clear();
                    last_emitted.clear();
                    let _ = command_tx.send(EngineCommand::TranscriptionFinished).await;
                }
                TranscriberMessage::Cancel => {
                    session_audio.clear();
                    last_emitted.clear();
                    let _ = command_tx.send(EngineCommand::TranscriptionFinished).await;
                }
                TranscriberMessage::UpdateModel(model) => {
                    if runtime.reload_model(model).is_err() {
                        continue;
                    }
                    session_audio.clear();
                    last_emitted.clear();
                }
            }
        }
    });
}

struct TranscriberRuntime {
    model_root: PathBuf,
    model: TranscriptionModel,
    context: WhisperContext,
}

impl TranscriberRuntime {
    fn new(model_root: PathBuf, model: TranscriptionModel) -> Result<Self> {
        let context = Self::load_context(&model_root, model)?;
        Ok(Self {
            model_root,
            model,
            context,
        })
    }

    fn reload_model(&mut self, model: TranscriptionModel) -> Result<()> {
        let context = Self::load_context(&self.model_root, model)?;
        self.model = model;
        self.context = context;
        Ok(())
    }

    fn load_context(model_root: &PathBuf, model: TranscriptionModel) -> Result<WhisperContext> {
        let model_path = model_root.join(model.file_name());
        if !model_path.exists() {
            anyhow::bail!("missing whisper model at {}", model_path.display());
        }

        let mut params = WhisperContextParameters::default();
        #[cfg(target_os = "macos")]
        {
            params.use_gpu(true);
            params.flash_attn(true);
        }

        WhisperContext::new_with_params(model_path.to_string_lossy().as_ref(), params)
            .with_context(|| format!("failed to load whisper model {}", model_path.display()))
    }

    fn transcribe(&self, samples_i16: &[i16], finalize: bool) -> Result<String> {
        if samples_i16.is_empty() {
            return Ok(String::new());
        }

        let mut samples = vec![0.0f32; samples_i16.len()];
        convert_integer_to_float_audio(samples_i16, &mut samples)
            .context("failed to convert audio to f32")?;

        let mut state = self.context.create_state().context("failed to create whisper state")?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(4);
        params.set_language(Some("en"));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_single_segment(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state.full(params, &samples).context("whisper inference failed")?;

        let mut raw = String::new();
        for segment in state.as_iter() {
            raw.push_str(segment.to_str_lossy()?.as_ref());
        }

        Ok(normalize_transcript(&raw, finalize))
    }
}

fn normalize_transcript(raw: &str, finalize: bool) -> String {
    let trimmed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return String::new();
    }

    let mut chars = trimmed.chars();
    let mut out = String::new();
    if let Some(first) = chars.next() {
        if first.is_ascii_alphabetic() {
            out.push(first.to_ascii_uppercase());
        } else {
            out.push(first);
        }
    }
    out.extend(chars);

    if finalize {
        let needs_terminal = !out.ends_with('.') && !out.ends_with('!') && !out.ends_with('?');
        if needs_terminal {
            out.push('.');
        }
    }

    out
}

fn transcript_delta(previous: &str, next: &str) -> String {
    if next.is_empty() {
        return String::new();
    }
    if previous.is_empty() {
        return next.to_string();
    }

    if let Some(suffix) = next.strip_prefix(previous) {
        return suffix.to_string();
    }

    let mut prefix_len = 0usize;
    for (a, b) in previous.chars().zip(next.chars()) {
        if a != b {
            break;
        }
        prefix_len += a.len_utf8();
    }

    next[prefix_len..].to_string()
}

#[cfg(test)]
mod tests {
    use super::{normalize_transcript, transcript_delta};

    #[test]
    fn normalize_adds_capitalization() {
        assert_eq!(normalize_transcript("hello world", false), "Hello world");
    }

    #[test]
    fn normalize_adds_terminal_punctuation() {
        assert_eq!(normalize_transcript("hello world", true), "Hello world.");
    }

    #[test]
    fn delta_only_emits_suffix() {
        assert_eq!(transcript_delta("Hello", "Hello world"), " world");
    }
}
