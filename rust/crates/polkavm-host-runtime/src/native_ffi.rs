/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::{
    ApplicationRuntime, AudioChunk, Frame, GpuBatch, InputEvent, InputEventType,
    PresentationProfile, TextInputKind, Tri2dFrame, UiOutputFrame, UiSemanticsFrame,
    INPUT_EVENT_BYTES,
};
#[cfg(feature = "native-gpu")]
use crate::{NativeGpuFrame, NativeGpuRenderer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmPresentationProfile {
    Framebuffer,
    Tri2d,
    WebGpuRaster,
    WebGpu,
}

impl From<NativePolkaVmPresentationProfile> for PresentationProfile {
    fn from(value: NativePolkaVmPresentationProfile) -> Self {
        match value {
            NativePolkaVmPresentationProfile::Framebuffer => Self::Framebuffer,
            NativePolkaVmPresentationProfile::Tri2d => Self::Tri2d,
            NativePolkaVmPresentationProfile::WebGpuRaster => Self::WebGpuRaster,
            NativePolkaVmPresentationProfile::WebGpu => Self::WebGpu,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmInputEventType {
    KeyDown,
    KeyUp,
    ButtonDown,
    ButtonUp,
    PointerMove,
    PointerDelta,
    SurfaceMetrics,
}

impl From<NativePolkaVmInputEventType> for InputEventType {
    fn from(value: NativePolkaVmInputEventType) -> Self {
        match value {
            NativePolkaVmInputEventType::KeyDown => Self::KeyDown,
            NativePolkaVmInputEventType::KeyUp => Self::KeyUp,
            NativePolkaVmInputEventType::ButtonDown => Self::ButtonDown,
            NativePolkaVmInputEventType::ButtonUp => Self::ButtonUp,
            NativePolkaVmInputEventType::PointerMove => Self::PointerMove,
            NativePolkaVmInputEventType::PointerDelta => Self::PointerDelta,
            NativePolkaVmInputEventType::SurfaceMetrics => Self::SurfaceMetrics,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmTextInputKind {
    Text,
    ImePreedit,
    ImeCommit,
}

impl From<NativePolkaVmTextInputKind> for TextInputKind {
    fn from(value: NativePolkaVmTextInputKind) -> Self {
        match value {
            NativePolkaVmTextInputKind::Text => Self::Text,
            NativePolkaVmTextInputKind::ImePreedit => Self::ImePreedit,
            NativePolkaVmTextInputKind::ImeCommit => Self::ImeCommit,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum NativePolkaVmMotionAvailability {
    Unavailable,
    Available,
    PermissionDenied,
}

impl From<NativePolkaVmMotionAvailability> for crate::motion_wire::MotionAvailability {
    fn from(value: NativePolkaVmMotionAvailability) -> Self {
        match value {
            NativePolkaVmMotionAvailability::Unavailable => Self::Unavailable,
            NativePolkaVmMotionAvailability::Available => Self::Available,
            NativePolkaVmMotionAvailability::PermissionDenied => Self::PermissionDenied,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmAsset {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmFrame {
    pub width: u32,
    pub height: u32,
    pub argb: Vec<u8>,
}

impl From<Frame> for NativePolkaVmFrame {
    fn from(frame: Frame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            argb: frame.argb,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmUiSemanticsFrame {
    pub bytes: Vec<u8>,
}

impl From<UiSemanticsFrame> for NativePolkaVmUiSemanticsFrame {
    fn from(frame: UiSemanticsFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmUiOutputFrame {
    pub bytes: Vec<u8>,
}

impl From<UiOutputFrame> for NativePolkaVmUiOutputFrame {
    fn from(frame: UiOutputFrame) -> Self {
        Self { bytes: frame.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmTri2dFrame {
    pub width: u32,
    pub height: u32,
    pub draw_count: u32,
    pub vertex_count: u32,
    pub index_count: u32,
    pub bytes: Vec<u8>,
}

impl From<Tri2dFrame> for NativePolkaVmTri2dFrame {
    fn from(frame: Tri2dFrame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            draw_count: frame.draw_count,
            vertex_count: frame.vertex_count,
            index_count: frame.index_count,
            bytes: frame.bytes,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmAudioChunk {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u32,
}

impl From<AudioChunk> for NativePolkaVmAudioChunk {
    fn from(chunk: AudioChunk) -> Self {
        Self {
            samples: chunk.samples,
            sample_rate: chunk.sample_rate,
            channels: chunk.channels,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmGpuBatch {
    pub bytes: Vec<u8>,
}

impl From<GpuBatch> for NativePolkaVmGpuBatch {
    fn from(batch: GpuBatch) -> Self {
        Self { bytes: batch.bytes }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NativePolkaVmGpuFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[cfg(feature = "native-gpu")]
impl From<NativeGpuFrame> for NativePolkaVmGpuFrame {
    fn from(frame: NativeGpuFrame) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba,
        }
    }
}

#[derive(Clone, Debug, thiserror::Error, uniffi::Error)]
pub enum NativePolkaVmError {
    #[error("{detail}")]
    Runtime { detail: String },
    #[error("asset path appears more than once: {path}")]
    DuplicateAsset { path: String },
    #[error("PolkaVM runtime mutex was poisoned")]
    RuntimePoisoned,
}

impl NativePolkaVmError {
    fn runtime(error: impl std::fmt::Display) -> Self {
        Self::Runtime {
            detail: error.to_string(),
        }
    }
}

#[derive(uniffi::Object)]
pub struct NativePolkaVmRuntime {
    runtime: Mutex<ApplicationRuntime>,
    #[cfg(feature = "native-gpu")]
    renderer: Mutex<Option<NativeGpuRenderer>>,
}

impl NativePolkaVmRuntime {
    fn lock(&self) -> Result<MutexGuard<'_, ApplicationRuntime>, NativePolkaVmError> {
        self.runtime
            .lock()
            .map_err(|_| NativePolkaVmError::RuntimePoisoned)
    }

    #[cfg(feature = "native-gpu")]
    fn renderer_lock(
        &self,
    ) -> Result<MutexGuard<'_, Option<NativeGpuRenderer>>, NativePolkaVmError> {
        self.renderer
            .lock()
            .map_err(|_| NativePolkaVmError::RuntimePoisoned)
    }
}

#[uniffi::export]
impl NativePolkaVmRuntime {
    #[uniffi::constructor]
    pub fn new(
        program: Vec<u8>,
        assets: Vec<NativePolkaVmAsset>,
        presentation: NativePolkaVmPresentationProfile,
        audio_enabled: bool,
        max_gas_per_update: u64,
    ) -> Result<Arc<Self>, NativePolkaVmError> {
        crate::validate_asset_count(assets.len()).map_err(NativePolkaVmError::runtime)?;
        let mut asset_map = HashMap::with_capacity(assets.len());
        for asset in assets {
            let path = asset.path;
            if asset_map.insert(path.clone(), asset.bytes).is_some() {
                return Err(NativePolkaVmError::DuplicateAsset { path });
            }
        }
        let runtime = ApplicationRuntime::new(
            &program,
            asset_map,
            presentation.into(),
            audio_enabled,
            max_gas_per_update,
        )
        .map_err(NativePolkaVmError::runtime)?;
        Ok(Arc::new(Self {
            runtime: Mutex::new(runtime),
            #[cfg(feature = "native-gpu")]
            renderer: Mutex::new(None),
        }))
    }

    pub fn init(&self) -> Result<(), NativePolkaVmError> {
        self.lock()?.init().map_err(NativePolkaVmError::runtime)
    }

    pub fn update(&self) -> Result<(), NativePolkaVmError> {
        self.lock()?.update().map_err(NativePolkaVmError::runtime)
    }

    pub fn backend(&self) -> Result<String, NativePolkaVmError> {
        Ok(format!("{:?}", self.lock()?.backend()).to_ascii_lowercase())
    }

    pub fn uses_motion(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.uses_motion())
    }

    pub fn last_gas_used(&self) -> Result<u64, NativePolkaVmError> {
        Ok(self.lock()?.last_gas_used())
    }

    pub fn send_input(
        &self,
        event_type: NativePolkaVmInputEventType,
        code: u8,
        x: u16,
        y: u16,
    ) -> Result<(), NativePolkaVmError> {
        self.lock()?.send_input(InputEvent {
            event_type: event_type.into(),
            code,
            x,
            y,
        });
        Ok(())
    }

    pub fn send_input_record(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        let record: [u8; INPUT_EVENT_BYTES] = bytes.try_into().map_err(|_| {
            NativePolkaVmError::runtime(format!(
                "input record must contain exactly {INPUT_EVENT_BYTES} bytes"
            ))
        })?;
        self.lock()?
            .send_input_record(record)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn send_text_input(
        &self,
        kind: NativePolkaVmTextInputKind,
        text: String,
    ) -> Result<(), NativePolkaVmError> {
        self.lock()?
            .send_text_input(kind.into(), &text)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn set_motion_availability(
        &self,
        availability: NativePolkaVmMotionAvailability,
    ) -> Result<(), NativePolkaVmError> {
        self.lock()?.set_motion_availability(availability.into());
        Ok(())
    }

    pub fn send_motion_sample(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock()?
            .send_motion_sample(&bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn uses_pointer_capture(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.uses_pointer_capture())
    }

    pub fn set_pointer_capture_supported(&self, supported: bool) -> Result<(), NativePolkaVmError> {
        self.lock()?.set_pointer_capture_supported(supported);
        Ok(())
    }

    pub fn set_pointer_capture_active(&self, active: bool) -> Result<(), NativePolkaVmError> {
        self.lock()?
            .set_pointer_capture_active(active)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn take_pointer_capture_request(&self) -> Result<Option<bool>, NativePolkaVmError> {
        Ok(self.lock()?.take_pointer_capture_request())
    }

    pub fn gpu_ready(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.gpu_ready())
    }

    pub fn set_gpu_capabilities(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock()?
            .set_gpu_capabilities(bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn send_gpu_event(&self, bytes: Vec<u8>) -> Result<(), NativePolkaVmError> {
        self.lock()?
            .send_gpu_event(bytes)
            .map_err(NativePolkaVmError::runtime)
    }

    pub fn configure_native_gpu(&self, width: u32, height: u32) -> Result<(), NativePolkaVmError> {
        #[cfg(feature = "native-gpu")]
        {
            let renderer =
                NativeGpuRenderer::new(width, height).map_err(NativePolkaVmError::runtime)?;
            let capabilities = renderer.capabilities();
            let mut runtime = self.lock()?;
            runtime
                .set_gpu_capabilities(capabilities)
                .map_err(NativePolkaVmError::runtime)?;
            *self.renderer_lock()? = Some(renderer);
            Ok(())
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            let _ = (width, height);
            Err(NativePolkaVmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    pub fn resize_native_gpu(&self, width: u32, height: u32) -> Result<(), NativePolkaVmError> {
        #[cfg(feature = "native-gpu")]
        {
            let mut runtime = self.lock()?;
            let mut renderer = self.renderer_lock()?;
            let renderer = renderer.as_mut().ok_or_else(|| {
                NativePolkaVmError::runtime("native GPU renderer is not configured")
            })?;
            renderer
                .resize(width, height)
                .map_err(NativePolkaVmError::runtime)?;
            runtime
                .set_gpu_capabilities(renderer.capabilities())
                .map_err(NativePolkaVmError::runtime)
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            let _ = (width, height);
            Err(NativePolkaVmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    pub fn render_native_gpu(&self) -> Result<Option<NativePolkaVmGpuFrame>, NativePolkaVmError> {
        #[cfg(feature = "native-gpu")]
        {
            let mut runtime = self.lock()?;
            let mut renderer = self.renderer_lock()?;
            let renderer = renderer.as_mut().ok_or_else(|| {
                NativePolkaVmError::runtime("native GPU renderer is not configured")
            })?;
            let mut frame = None;
            while let Some(batch) = runtime.take_gpu_batch() {
                let rendered = renderer.execute(&batch.bytes);
                for event in rendered.events {
                    runtime
                        .send_gpu_event(event)
                        .map_err(NativePolkaVmError::runtime)?;
                }
                if let Some(rendered_frame) = rendered.frame {
                    frame = Some(rendered_frame.into());
                }
            }
            Ok(frame)
        }
        #[cfg(not(feature = "native-gpu"))]
        {
            Err(NativePolkaVmError::runtime(
                "native GPU support is not included in this host build",
            ))
        }
    }

    pub fn take_frame(&self) -> Result<Option<NativePolkaVmFrame>, NativePolkaVmError> {
        Ok(self.lock()?.take_frame().map(Into::into))
    }

    pub fn take_tri2d(&self) -> Result<Option<NativePolkaVmTri2dFrame>, NativePolkaVmError> {
        Ok(self.lock()?.take_tri2d().map(Into::into))
    }

    pub fn take_audio(&self) -> Result<Option<NativePolkaVmAudioChunk>, NativePolkaVmError> {
        Ok(self.lock()?.take_audio().map(Into::into))
    }

    pub fn take_gpu_batch(&self) -> Result<Option<NativePolkaVmGpuBatch>, NativePolkaVmError> {
        Ok(self.lock()?.take_gpu_batch().map(Into::into))
    }

    pub fn take_ui_semantics(
        &self,
    ) -> Result<Option<NativePolkaVmUiSemanticsFrame>, NativePolkaVmError> {
        Ok(self.lock()?.take_ui_semantics().map(Into::into))
    }

    pub fn take_ui_output(&self) -> Result<Option<NativePolkaVmUiOutputFrame>, NativePolkaVmError> {
        Ok(self.lock()?.take_ui_output().map(Into::into))
    }

    pub fn take_log(&self) -> Result<Option<String>, NativePolkaVmError> {
        Ok(self.lock()?.take_log())
    }

    pub fn is_exited(&self) -> Result<bool, NativePolkaVmError> {
        Ok(self.lock()?.is_exited())
    }

    pub fn take_save(&self) -> Result<Option<Vec<u8>>, NativePolkaVmError> {
        Ok(self.lock()?.take_save())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_assets_are_rejected_before_program_parsing() {
        let result = NativePolkaVmRuntime::new(
            Vec::new(),
            vec![
                NativePolkaVmAsset {
                    path: "data.bin".into(),
                    bytes: vec![1],
                },
                NativePolkaVmAsset {
                    path: "data.bin".into(),
                    bytes: vec![2],
                },
            ],
            NativePolkaVmPresentationProfile::Framebuffer,
            false,
            1,
        );
        assert!(matches!(
            result,
            Err(NativePolkaVmError::DuplicateAsset { .. })
        ));
    }

    #[test]
    fn text_input_kinds_match_the_runtime_contract() {
        assert_eq!(
            TextInputKind::from(NativePolkaVmTextInputKind::Text),
            TextInputKind::Text
        );
        assert_eq!(
            TextInputKind::from(NativePolkaVmTextInputKind::ImePreedit),
            TextInputKind::ImePreedit
        );
        assert_eq!(
            TextInputKind::from(NativePolkaVmTextInputKind::ImeCommit),
            TextInputKind::ImeCommit
        );
    }
}
