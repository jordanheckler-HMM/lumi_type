use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::{state::EngineCommand, AudioFrame};

pub struct AudioCapture {
    _stream: cpal::Stream,
}

impl AudioCapture {
    pub fn start(
        command_tx: mpsc::Sender<EngineCommand>,
        preferred_device: Option<String>,
    ) -> Result<Self> {
        let host = cpal::default_host();
        let device = select_device(&host, preferred_device)?;
        let config = device
            .default_input_config()
            .context("failed to read default input config")?;

        let channels = config.channels() as usize;
        let sample_rate = config.sample_rate().0;
        let frame_samples = ((sample_rate as f32) * 0.02) as usize;
        let sample_buffer = Arc::new(Mutex::new(Vec::<i16>::with_capacity(frame_samples * 3)));

        let stream_config: cpal::StreamConfig = config.clone().into();
        let stream = match config.sample_format() {
            cpal::SampleFormat::I16 => build_stream_i16(
                &device,
                &stream_config,
                channels,
                sample_rate,
                frame_samples,
                sample_buffer,
                command_tx,
            )?,
            cpal::SampleFormat::U16 => build_stream_u16(
                &device,
                &stream_config,
                channels,
                sample_rate,
                frame_samples,
                sample_buffer,
                command_tx,
            )?,
            cpal::SampleFormat::F32 => build_stream_f32(
                &device,
                &stream_config,
                channels,
                sample_rate,
                frame_samples,
                sample_buffer,
                command_tx,
            )?,
            other => {
                return Err(anyhow!("unsupported audio sample format: {other:?}"));
            }
        };

        stream.play().context("failed to start input stream")?;

        Ok(Self { _stream: stream })
    }
}

fn select_device(host: &cpal::Host, preferred_device: Option<String>) -> Result<cpal::Device> {
    if let Some(name) = preferred_device {
        if !name.is_empty() {
            let mut devices = host
                .input_devices()
                .context("failed to enumerate input devices")?;
            if let Some(device) = devices.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                return Ok(device);
            }
        }
    }

    host.default_input_device()
        .ok_or_else(|| anyhow!("no default input device available"))
}

fn build_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sample_rate: u32,
    frame_samples: usize,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    command_tx: mpsc::Sender<EngineCommand>,
) -> Result<cpal::Stream> {
    let err_fn = |_err| {};

    let stream = device
        .build_input_stream(
            config,
            move |input: &[i16], _| {
                push_mono_samples(
                    input,
                    channels,
                    sample_rate,
                    frame_samples,
                    &sample_buffer,
                    &command_tx,
                    |sample| sample,
                );
            },
            err_fn,
            None,
        )
        .context("failed to build i16 stream")?;

    Ok(stream)
}

fn build_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sample_rate: u32,
    frame_samples: usize,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    command_tx: mpsc::Sender<EngineCommand>,
) -> Result<cpal::Stream> {
    let err_fn = |_err| {};

    let stream = device
        .build_input_stream(
            config,
            move |input: &[u16], _| {
                push_mono_samples(
                    input,
                    channels,
                    sample_rate,
                    frame_samples,
                    &sample_buffer,
                    &command_tx,
                    |sample| (sample as i32 - 32768) as i16,
                );
            },
            err_fn,
            None,
        )
        .context("failed to build u16 stream")?;

    Ok(stream)
}

fn build_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sample_rate: u32,
    frame_samples: usize,
    sample_buffer: Arc<Mutex<Vec<i16>>>,
    command_tx: mpsc::Sender<EngineCommand>,
) -> Result<cpal::Stream> {
    let err_fn = |_err| {};

    let stream = device
        .build_input_stream(
            config,
            move |input: &[f32], _| {
                push_mono_samples(
                    input,
                    channels,
                    sample_rate,
                    frame_samples,
                    &sample_buffer,
                    &command_tx,
                    |sample| {
                        (sample.clamp(-1.0, 1.0) * i16::MAX as f32)
                            .round()
                            .clamp(i16::MIN as f32, i16::MAX as f32)
                            as i16
                    },
                );
            },
            err_fn,
            None,
        )
        .context("failed to build f32 stream")?;

    Ok(stream)
}

fn push_mono_samples<T, F>(
    input: &[T],
    channels: usize,
    sample_rate: u32,
    frame_samples: usize,
    sample_buffer: &Arc<Mutex<Vec<i16>>>,
    command_tx: &mpsc::Sender<EngineCommand>,
    convert: F,
) where
    T: Copy,
    F: Fn(T) -> i16,
{
    let mut mono = Vec::with_capacity(input.len() / channels.max(1));

    for frame in input.chunks(channels.max(1)) {
        let mut acc: i32 = 0;
        let mut count: i32 = 0;

        for sample in frame {
            acc += convert(*sample) as i32;
            count += 1;
        }

        if count == 0 {
            continue;
        }

        mono.push((acc / count) as i16);
    }

    let mut guard = sample_buffer.lock();
    guard.extend_from_slice(&mono);

    while guard.len() >= frame_samples {
        let frame: Vec<i16> = guard.drain(..frame_samples).collect();
        let peak = frame
            .iter()
            .map(|s| (*s as f32).abs() / i16::MAX as f32)
            .fold(0.0f32, f32::max);

        let _ = command_tx.try_send(EngineCommand::AudioFrame(AudioFrame {
            samples: frame,
            sample_rate,
            peak,
        }));
    }
}
