/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::corevm::{Interruption, Vm};
use crate::{
    AudioChunk, ComputerContext, Frame, GpuBatch, InputEvent, InputEventType, PresentationProfile,
    Runtime, TextInputKind, Tri2dFrame, UiOutputFrame, UiSemanticsFrame, INPUT_EVENT_BYTES,
    MAX_FRAME_BYTES,
};
use anyhow::{anyhow, Context, Result};
use polkavm::ProgramBlob;
use std::collections::{HashMap, VecDeque};

const MAX_INTERRUPTS_PER_UPDATE: usize = 8_192;
const MAX_QUEUED_AUDIO_CHUNKS: usize = 64;

// Both variants are large, long-lived runtime state machines. Boxing either
// adds allocation and indirection to every host call to save 576 enum bytes.
#[allow(clippy::large_enum_variant)]
pub enum ApplicationRuntime {
    Cooperative(Runtime),
    CoreVm(CoreVmRuntime),
}

pub struct CoreVmRuntime {
    vm: Vm,
    frame: Option<Frame>,
    audio: VecDeque<AudioChunk>,
    palette: [[u8; 3]; 256],
    sample_rate: u32,
    channels: u32,
    pointer: Option<(u16, u16)>,
    audio_enabled: bool,
    max_gas_per_update: u64,
    exited: bool,
}

impl ApplicationRuntime {
    pub fn new(
        program: &[u8],
        assets: HashMap<String, Vec<u8>>,
        presentation: PresentationProfile,
        audio_enabled: bool,
        max_gas_per_update: u64,
    ) -> Result<Self> {
        Self::new_with_backend(
            program,
            assets,
            presentation,
            audio_enabled,
            max_gas_per_update,
            crate::preferred_backend(),
        )
    }

    pub fn new_with_backend(
        program: &[u8],
        assets: HashMap<String, Vec<u8>>,
        presentation: PresentationProfile,
        audio_enabled: bool,
        max_gas_per_update: u64,
        backend: polkavm::BackendKind,
    ) -> Result<Self> {
        crate::validate_launch_inputs(program, &assets, max_gas_per_update)?;
        let blob = ProgramBlob::parse(program.into()).context("parse PolkaVM program")?;
        crate::validate_blob(&blob)?;
        let is_corevm = blob.exports().any(|export| export.symbol() == "_pvm_start");
        if !is_corevm {
            return Runtime::from_blob(
                blob,
                assets,
                presentation,
                audio_enabled,
                max_gas_per_update,
                backend,
            )
            .map(Self::Cooperative);
        }
        if presentation != PresentationProfile::Framebuffer {
            return Err(anyhow!(
                "CoreVM guests require the framebuffer presentation profile"
            ));
        }

        let mut vm = Vm::from_blob(blob, backend).context("create CoreVM guest")?;
        for (path, bytes) in assets {
            vm.register_file(&path, bytes);
        }
        let context = ComputerContext::new(vec!["./quake".into()], Vec::new())?;
        vm.setup(context).map_err(|error| anyhow!(error))?;
        Ok(Self::CoreVm(CoreVmRuntime {
            vm,
            frame: None,
            audio: VecDeque::new(),
            palette: [[255; 3]; 256],
            sample_rate: 0,
            channels: 0,
            audio_enabled,
            pointer: None,
            max_gas_per_update,
            exited: false,
        }))
    }

    pub fn init(&mut self) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.init(),
            Self::CoreVm(_) => Ok(()),
        }
    }

    pub fn update(&mut self) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.update(),
            Self::CoreVm(runtime) => runtime.update(),
        }
    }

    pub fn backend(&self) -> polkavm::BackendKind {
        match self {
            Self::Cooperative(runtime) => runtime.backend(),
            Self::CoreVm(runtime) => runtime.vm.backend(),
        }
    }

    pub fn uses_motion(&self) -> bool {
        match self {
            Self::Cooperative(runtime) => runtime.uses_motion(),
            Self::CoreVm(runtime) => runtime.vm.uses_motion(),
        }
    }

    pub fn uses_pointer_capture(&self) -> bool {
        match self {
            Self::Cooperative(runtime) => runtime.uses_pointer_capture(),
            Self::CoreVm(runtime) => runtime.vm.uses_pointer_capture(),
        }
    }

    pub fn set_pointer_capture_supported(&mut self, supported: bool) {
        match self {
            Self::Cooperative(runtime) => runtime.set_pointer_capture_supported(supported),
            Self::CoreVm(runtime) => runtime.vm.set_pointer_capture_supported(supported),
        }
    }

    pub fn set_pointer_capture_active(&mut self, active: bool) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.set_pointer_capture_active(active),
            Self::CoreVm(runtime) => runtime
                .vm
                .set_pointer_capture_active(active)
                .map_err(anyhow::Error::msg),
        }
    }

    pub fn take_pointer_capture_request(&mut self) -> Option<bool> {
        match self {
            Self::Cooperative(runtime) => runtime.take_pointer_capture_request(),
            Self::CoreVm(runtime) => runtime.vm.take_pointer_capture_request(),
        }
    }

    pub fn last_gas_used(&self) -> u64 {
        match self {
            Self::Cooperative(runtime) => runtime.last_gas_used(),
            Self::CoreVm(runtime) => runtime
                .max_gas_per_update
                .saturating_sub(runtime.vm.gas_remaining()),
        }
    }

    pub fn send_input(&mut self, event: InputEvent) {
        match self {
            Self::Cooperative(runtime) => runtime.send_input(event),
            Self::CoreVm(runtime) => runtime.send_input(event),
        }
    }

    pub fn send_input_record(&mut self, record: [u8; INPUT_EVENT_BYTES]) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.send_input_record(record),
            Self::CoreVm(_) => Err(anyhow!("CoreVM does not support extended input records")),
        }
    }

    pub fn send_text_input(&mut self, kind: TextInputKind, text: &str) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.send_text_input(kind, text),
            Self::CoreVm(_) => Err(anyhow!("CoreVM does not support text input")),
        }
    }

    pub fn set_motion_availability(
        &mut self,
        availability: crate::motion_wire::MotionAvailability,
    ) {
        match self {
            Self::Cooperative(runtime) => runtime.set_motion_availability(availability),
            Self::CoreVm(runtime) => runtime.vm.set_motion_availability(availability),
        }
    }

    pub fn send_motion_sample(&mut self, bytes: &[u8]) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.send_motion_sample(bytes),
            Self::CoreVm(runtime) => runtime
                .vm
                .send_motion_sample(bytes)
                .map_err(anyhow::Error::msg),
        }
    }

    pub fn gpu_ready(&self) -> bool {
        match self {
            Self::Cooperative(runtime) => runtime.gpu_ready(),
            Self::CoreVm(_) => true,
        }
    }

    pub fn set_gpu_capabilities(&mut self, bytes: Vec<u8>) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.set_gpu_capabilities(bytes),
            Self::CoreVm(_) => Err(anyhow!("CoreVM does not support GPU capabilities")),
        }
    }

    pub fn send_gpu_event(&mut self, bytes: Vec<u8>) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.send_gpu_event(bytes),
            Self::CoreVm(_) => Err(anyhow!("CoreVM does not support GPU events")),
        }
    }

    pub fn take_gpu_batch(&mut self) -> Option<GpuBatch> {
        match self {
            Self::Cooperative(runtime) => runtime.take_gpu_batch(),
            Self::CoreVm(_) => None,
        }
    }

    pub fn take_host_frame_request(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::Cooperative(runtime) => runtime.take_host_frame_request(),
            Self::CoreVm(runtime) => runtime.vm.take_host_frame_request(),
        }
    }

    pub fn send_host_frame_response(&mut self, bytes: Vec<u8>) -> Result<()> {
        match self {
            Self::Cooperative(runtime) => runtime.send_host_frame_response(bytes),
            Self::CoreVm(runtime) => runtime
                .vm
                .send_host_frame_response(bytes)
                .map_err(anyhow::Error::msg),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_time_ms(&mut self, time_ms: u64) {
        match self {
            Self::Cooperative(runtime) => runtime.set_time_ms(time_ms),
            Self::CoreVm(runtime) => runtime.vm.set_time_ms(time_ms),
        }
    }

    pub fn take_frame(&mut self) -> Option<Frame> {
        match self {
            Self::Cooperative(runtime) => runtime.take_frame(),
            Self::CoreVm(runtime) => runtime.frame.take(),
        }
    }
    pub fn take_tri2d(&mut self) -> Option<Tri2dFrame> {
        match self {
            Self::Cooperative(runtime) => runtime.take_tri2d(),
            Self::CoreVm(_) => None,
        }
    }

    pub fn take_ui_semantics(&mut self) -> Option<UiSemanticsFrame> {
        match self {
            Self::Cooperative(runtime) => runtime.take_ui_semantics(),
            Self::CoreVm(_) => None,
        }
    }

    pub fn take_ui_output(&mut self) -> Option<UiOutputFrame> {
        match self {
            Self::Cooperative(runtime) => runtime.take_ui_output(),
            Self::CoreVm(_) => None,
        }
    }

    pub fn take_audio(&mut self) -> Option<AudioChunk> {
        match self {
            Self::Cooperative(runtime) => runtime.take_audio(),
            Self::CoreVm(runtime) => runtime.audio.pop_front(),
        }
    }

    pub fn take_log(&mut self) -> Option<String> {
        match self {
            Self::Cooperative(runtime) => runtime.take_log(),
            Self::CoreVm(_) => None,
        }
    }

    pub fn is_exited(&self) -> bool {
        matches!(self, Self::CoreVm(runtime) if runtime.exited)
    }

    pub fn take_save(&mut self) -> Option<Vec<u8>> {
        match self {
            Self::Cooperative(runtime) => runtime.take_save(),
            Self::CoreVm(_) => None,
        }
    }
}

impl CoreVmRuntime {
    fn update(&mut self) -> Result<()> {
        if self.exited {
            return Ok(());
        }
        self.vm.set_gas(self.max_gas_per_update);
        for _ in 0..MAX_INTERRUPTS_PER_UPDATE {
            match self.vm.run().map_err(|error| anyhow!(error))? {
                Interruption::Exit(status) => {
                    if status != 0 {
                        return Err(anyhow!("exit called with status: {status}"));
                    }
                    self.exited = true;
                    return Ok(());
                }
                Interruption::ProcessRun { package, .. }
                | Interruption::ProcessSpawn { package, .. } => {
                    return Err(anyhow!(
                        "CoreVM guest requested process spawn of {package:?}"
                    ));
                }
                Interruption::ProcessWait { .. }
                | Interruption::PipeRead { .. }
                | Interruption::PipeWrite { .. }
                | Interruption::PipeClose { .. } => {
                    return Err(anyhow!("CoreVM guest requested a pipe operation"));
                }
                Interruption::WorkspaceSpawn { .. }
                | Interruption::WorkspaceSendInput { .. }
                | Interruption::WorkspaceRead { .. }
                | Interruption::WorkspaceResize { .. }
                | Interruption::WorkspaceWait { .. }
                | Interruption::WorkspaceClose { .. } => {
                    return Err(anyhow!("CoreVM guest requested a workspace operation"));
                }
                Interruption::Yield => return Ok(()),
                Interruption::SetPalette { palette } => {
                    if palette.len() != 256 * 3 {
                        return Err(anyhow!("guest supplied an invalid Quake palette"));
                    }
                    for (target, source) in self.palette.iter_mut().zip(palette.as_chunks::<3>().0)
                    {
                        target.copy_from_slice(source);
                    }
                }
                Interruption::Display {
                    width,
                    height,
                    framebuffer,
                } => {
                    let width = u32::try_from(width).context("Quake frame width overflow")?;
                    let height = u32::try_from(height).context("Quake frame height overflow")?;
                    let pixels = width
                        .checked_mul(height)
                        .ok_or_else(|| anyhow!("Quake frame dimensions overflow"))?
                        as usize;
                    if pixels == 0 || pixels > MAX_FRAME_BYTES / 4 || framebuffer.len() != pixels {
                        return Err(anyhow!("guest supplied an invalid Quake frame"));
                    }
                    let mut argb = Vec::with_capacity(pixels * 4);
                    for index in framebuffer {
                        let [red, green, blue] = self.palette[index as usize];
                        argb.extend_from_slice(&[blue, green, red, 255]);
                    }
                    self.frame = Some(Frame {
                        width,
                        height,
                        argb,
                    });
                    return Ok(());
                }
                Interruption::AudioInit {
                    channels,
                    sample_rate,
                } => {
                    if !self.audio_enabled {
                        self.channels = 0;
                        self.sample_rate = 0;
                        continue;
                    }
                    if !(1..=2).contains(&channels) || !(8_000..=96_000).contains(&sample_rate) {
                        return Err(anyhow!("guest requested an unsupported audio format"));
                    }
                    self.channels = channels;
                    self.sample_rate = sample_rate;
                }
                Interruption::AudioFrame { buffer } => {
                    if !self.audio_enabled {
                        continue;
                    }
                    if self.channels == 0 {
                        self.channels = crate::AUDIO_CHANNELS;
                        self.sample_rate = crate::AUDIO_SAMPLE_RATE;
                    }
                    if buffer.is_empty() || buffer.len() % self.channels as usize != 0 {
                        continue;
                    }
                    if self.audio.len() == MAX_QUEUED_AUDIO_CHUNKS {
                        self.audio.pop_front();
                    }
                    self.audio.push_back(AudioChunk {
                        samples: buffer,
                        sample_rate: self.sample_rate,
                        channels: self.channels,
                    });
                }
            }
        }
        Err(anyhow!("guest exceeded interruption budget"))
    }

    fn send_input(&mut self, event: InputEvent) {
        let pointer_delta = if event.event_type == InputEventType::PointerDelta {
            match (corevm_pointer_delta(event.x), corevm_pointer_delta(event.y)) {
                (Some(delta_x), Some(delta_y)) => Some((delta_x, delta_y)),
                _ => return,
            }
        } else {
            None
        };
        if self.vm.uses_epoca_inputs() {
            self.vm.send_epoca_input(event);
            return;
        }
        match event.event_type {
            InputEventType::KeyDown | InputEventType::KeyUp => {
                if let Some(key) = crate::quake_keys::from_hid(event.code) {
                    self.vm
                        .send_key(key, event.event_type == InputEventType::KeyDown);
                }
            }
            InputEventType::ButtonDown | InputEventType::ButtonUp => {
                if let Some(key) = crate::quake_keys::from_button(event.code) {
                    self.vm
                        .send_key(key, event.event_type == InputEventType::ButtonDown);
                }
            }
            InputEventType::PointerMove => {
                if let Some((previous_x, previous_y)) = self.pointer {
                    self.vm.send_mouse_move(
                        signed_delta(event.x, previous_x),
                        signed_delta(event.y, previous_y),
                    );
                }
                self.pointer = Some((event.x, event.y));
            }
            InputEventType::PointerDelta => {
                if let Some((delta_x, delta_y)) = pointer_delta {
                    self.vm.send_mouse_move(delta_x, delta_y);
                }
            }
            InputEventType::SurfaceMetrics => {}
        }
    }
}

fn signed_delta(current: u16, previous: u16) -> i8 {
    (i32::from(current) - i32::from(previous)).clamp(i8::MIN as i32, i8::MAX as i32) as i8
}

fn corevm_pointer_delta(value: u16) -> Option<i8> {
    i8::try_from(value as i16).ok()
}

#[cfg(test)]
mod tests {
    use super::corevm_pointer_delta;

    #[test]
    fn corevm_pointer_delta_preserves_representable_signed_values() {
        assert_eq!(corevm_pointer_delta(127), Some(127));
        assert_eq!(corevm_pointer_delta((-128_i16) as u16), Some(-128));
    }

    #[test]
    fn corevm_pointer_delta_drops_safari_pointer_lock_discontinuities() {
        assert_eq!(corevm_pointer_delta(128), None);
        assert_eq!(corevm_pointer_delta((-129_i16) as u16), None);
        assert_eq!(corevm_pointer_delta(430), None);
    }
}
