use std::{
    ffi::CString,
    os::raw::{c_char, c_int, c_void},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use libloading::Library;
use tokio::sync::mpsc;

use super::{
    state::EngineCommand,
    vad::resample_mono_to_16k,
    AudioFrame,
};

#[derive(Debug, Clone)]
pub struct WakeWordConfig {
    pub porcupine_library: PathBuf,
    pub model_path: PathBuf,
    pub keyword_path: PathBuf,
    pub keyword_fallback_path: Option<PathBuf>,
    pub sensitivity: f32,
}

impl WakeWordConfig {
    pub fn from_model_root(model_root: &Path, sensitivity: f32) -> Self {
        Self {
            porcupine_library: default_porcupine_library_path(),
            model_path: model_root.join("porcupine_params.pv"),
            keyword_path: model_root.join("hey-lumi-mac.ppn"),
            keyword_fallback_path: Some(model_root.join("porcupine_mac.ppn")),
            sensitivity,
        }
    }

    pub fn with_overrides_from_env(mut self) -> Self {
        if let Ok(value) = std::env::var("LUMI_PORCUPINE_DYLIB") {
            self.porcupine_library = PathBuf::from(value);
        }
        if let Ok(value) = std::env::var("LUMI_PORCUPINE_MODEL") {
            self.model_path = PathBuf::from(value);
        }
        if let Ok(value) = std::env::var("LUMI_PORCUPINE_KEYWORD") {
            self.keyword_path = PathBuf::from(value);
            self.keyword_fallback_path = None;
        }
        if let Ok(value) = std::env::var("LUMI_PORCUPINE_FALLBACK_KEYWORD") {
            self.keyword_fallback_path = Some(PathBuf::from(value));
        }
        self
    }
}

pub fn spawn_wake_listener(
    mut rx: mpsc::Receiver<AudioFrame>,
    command_tx: mpsc::Sender<EngineCommand>,
    config: WakeWordConfig,
) {
    tauri::async_runtime::spawn(async move {
        let mut detector = match PorcupineDetector::new(&config) {
            Ok(detector) => detector,
            Err(error) => {
                eprintln!("wake-word disabled: {error}");
                return;
            }
        };
        if detector.keyword_path() != config.keyword_path.as_path() {
            eprintln!(
                "wake-word fallback active (using {} instead of {})",
                detector.keyword_path().display(),
                config.keyword_path.display()
            );
        }

        while let Some(frame) = rx.recv().await {
            if detector.process_frame(&frame).unwrap_or(false) {
                let _ = command_tx.send(EngineCommand::WakeDetected).await;
            }
        }
    });
}

fn default_porcupine_library_path() -> PathBuf {
    let arm_path = PathBuf::from("/opt/homebrew/lib/libpv_porcupine.dylib");
    if arm_path.exists() {
        return arm_path;
    }

    PathBuf::from("/usr/local/lib/libpv_porcupine.dylib")
}

type PorcupineInitFn = unsafe extern "C" fn(
    model_file_path: *const c_char,
    keyword_file_path: *const c_char,
    sensitivity: f32,
    object_out: *mut *mut c_void,
) -> c_int;

type PorcupineFrameLengthFn = unsafe extern "C" fn() -> c_int;
type PorcupineProcessFn = unsafe extern "C" fn(
    object: *mut c_void,
    pcm: *const i16,
    is_wake_word_detected: *mut bool,
) -> c_int;
type PorcupineDeleteFn = unsafe extern "C" fn(object: *mut c_void);

struct PorcupineDetector {
    _library: Library,
    object: *mut c_void,
    keyword_path: PathBuf,
    frame_length: usize,
    process: PorcupineProcessFn,
    delete: PorcupineDeleteFn,
    frame_buffer: Vec<i16>,
}

unsafe impl Send for PorcupineDetector {}

impl PorcupineDetector {
    fn new(config: &WakeWordConfig) -> Result<Self> {
        if !config.model_path.exists() {
            anyhow::bail!("missing Porcupine model file at {}", config.model_path.display());
        }

        let keyword_path = if config.keyword_path.exists() {
            config.keyword_path.clone()
        } else if let Some(fallback) = config.keyword_fallback_path.as_ref().filter(|p| p.exists()) {
            fallback.clone()
        } else {
            match &config.keyword_fallback_path {
                Some(fallback) => {
                    anyhow::bail!(
                        "missing wake keyword files at {} and {}",
                        config.keyword_path.display(),
                        fallback.display()
                    );
                }
                None => {
                    anyhow::bail!("missing wake keyword file at {}", config.keyword_path.display());
                }
            }
        };

        let library = unsafe { Library::new(&config.porcupine_library) }
            .with_context(|| format!("unable to load Porcupine dylib at {}", config.porcupine_library.display()))?;

        let init: PorcupineInitFn = unsafe {
            *library
                .get(b"pv_porcupine_init\0")
                .context("missing symbol pv_porcupine_init")?
        };
        let frame_length_fn: PorcupineFrameLengthFn = unsafe {
            *library
                .get(b"pv_porcupine_frame_length\0")
                .context("missing symbol pv_porcupine_frame_length")?
        };
        let process: PorcupineProcessFn = unsafe {
            *library
                .get(b"pv_porcupine_process\0")
                .context("missing symbol pv_porcupine_process")?
        };
        let delete: PorcupineDeleteFn = unsafe {
            *library
                .get(b"pv_porcupine_delete\0")
                .context("missing symbol pv_porcupine_delete")?
        };

        let model = CString::new(config.model_path.to_string_lossy().to_string())?;
        let keyword = CString::new(keyword_path.to_string_lossy().to_string())?;
        let mut object = std::ptr::null_mut();
        let status = unsafe {
            init(
                model.as_ptr(),
                keyword.as_ptr(),
                config.sensitivity.clamp(0.0, 1.0),
                &mut object,
            )
        };
        if status != 0 || object.is_null() {
            anyhow::bail!("Porcupine init failed with status {status}");
        }

        let frame_length = unsafe { frame_length_fn() as usize };

        Ok(Self {
            _library: library,
            object,
            keyword_path,
            frame_length,
            process,
            delete,
            frame_buffer: Vec::with_capacity(frame_length * 2),
        })
    }

    fn keyword_path(&self) -> &Path {
        &self.keyword_path
    }

    fn process_frame(&mut self, frame: &AudioFrame) -> Result<bool> {
        let resampled = resample_mono_to_16k(&frame.samples, frame.sample_rate);
        self.frame_buffer.extend_from_slice(&resampled);

        while self.frame_buffer.len() >= self.frame_length {
            let pcm = &self.frame_buffer[..self.frame_length];
            let mut detected = false;
            let status = unsafe { (self.process)(self.object, pcm.as_ptr(), &mut detected) };
            self.frame_buffer.drain(..self.frame_length);
            if status != 0 {
                anyhow::bail!("Porcupine process failed with status {status}");
            }
            if detected {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

impl Drop for PorcupineDetector {
    fn drop(&mut self) {
        unsafe {
            (self.delete)(self.object);
        }
    }
}
