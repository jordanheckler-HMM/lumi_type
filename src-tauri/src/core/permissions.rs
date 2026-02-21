use serde::Serialize;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PermissionStatus {
    pub microphone: bool,
    pub accessibility: bool,
}

impl PermissionStatus {
    pub fn all_granted(self) -> bool {
        self.microphone && self.accessibility
    }
}

pub fn check_permissions() -> PermissionStatus {
    PermissionStatus {
        microphone: microphone_permission_granted(),
        accessibility: accessibility_permission_granted(),
    }
}

pub fn request_permissions() -> PermissionStatus {
    let accessibility = request_accessibility_permission();
    let microphone = microphone_permission_granted();
    PermissionStatus {
        microphone,
        accessibility,
    }
}

pub fn accessibility_permission_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_accessibility_client::accessibility::application_is_trusted()
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn request_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_accessibility_client::accessibility::application_is_trusted_with_prompt()
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn microphone_permission_granted() -> bool {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        return false;
    };

    let Ok(config) = device.default_input_config() else {
        return false;
    };

    let stream_config = config.config();
    let err_fn = |_err| {};

    let stream_result = match config.sample_format() {
        cpal::SampleFormat::I8 => device.build_input_stream(&stream_config, |_data: &[i8], _| {}, err_fn, None),
        cpal::SampleFormat::I16 => device.build_input_stream(&stream_config, |_data: &[i16], _| {}, err_fn, None),
        cpal::SampleFormat::I32 => device.build_input_stream(&stream_config, |_data: &[i32], _| {}, err_fn, None),
        cpal::SampleFormat::F32 => device.build_input_stream(&stream_config, |_data: &[f32], _| {}, err_fn, None),
        _ => return false,
    };

    let Ok(stream) = stream_result else {
        return false;
    };

    stream.play().is_ok()
}
