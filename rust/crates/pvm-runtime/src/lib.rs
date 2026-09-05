/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#[cfg(target_arch = "wasm32")]
extern crate polkavm_wasm as polkavm;

mod application;
mod computer;
mod corevm;
mod filesystem;
pub use pvm_gpu_wire as gpu_wire;
pub use pvm_motion_wire as motion_wire;
pub use pvm_ui_wire as ui_wire;
mod manifest;
#[cfg(all(not(target_arch = "wasm32"), feature = "ffi"))]
mod native_ffi;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-gpu"))]
mod native_gpu;
mod quake_keys;
mod tri2d;
mod ui;
#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(any(target_arch = "wasm32", test))]
mod wasm_codegen;

#[cfg(all(not(target_arch = "wasm32"), feature = "ffi"))]
pub use native_ffi::*;
#[cfg(all(not(target_arch = "wasm32"), feature = "native-gpu"))]
pub use native_gpu::*;

#[cfg(all(not(target_arch = "wasm32"), feature = "ffi"))]
uniffi::setup_scaffolding!();

pub use application::ApplicationRuntime;
pub use computer::{
    ChildProcessRequest, ComputerContext, ComputerRuntime, ComputerStatus, ComputerSupervisor,
    COMPUTER_ABI_VERSION, COMPUTER_TTY_HANDLE, FS_OPEN_APPEND, FS_OPEN_CREATE, FS_OPEN_EXCLUSIVE,
    FS_OPEN_READ, FS_OPEN_TRUNCATE, FS_OPEN_WRITE, MAX_BACKGROUND_PROCESSES,
    MAX_COMPUTER_CONTEXT_BYTES, MAX_COMPUTER_CONTEXT_ENTRIES, MAX_COMPUTER_DIRECTORIES,
    MAX_COMPUTER_FILES, MAX_COMPUTER_FILE_BYTES, MAX_COMPUTER_PATH_BYTES, MAX_COMPUTER_PROCESSES,
    MAX_NET_ADDRESS_BYTES, MAX_OPEN_COMPUTER_FILES, MAX_OPEN_SOCKETS, MAX_TTY_INPUT_BYTES,
    MAX_TTY_OUTPUT_BYTES, MAX_WORKSPACE_CHILDREN, TTY_MODE_ECHO, TTY_MODE_RAW,
};
pub use filesystem::{FilesystemMetadata, FilesystemMetadataEntry};
pub use manifest::AppDescriptor;

use anyhow::{anyhow, bail, Context, Result};
pub use polkavm::BackendKind;
use polkavm::{CallError, Config, Engine, Instance, Linker, Module, ProgramBlob};
use std::collections::{HashMap, VecDeque};
use std::mem::size_of;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
pub use tri2d::{
    Tri2dFrame, MAX_TRI2D_BYTES, MAX_TRI2D_COMMANDS, MAX_TRI2D_DRAWS, MAX_TRI2D_INDICES,
    MAX_TRI2D_SURFACE_SIZE, MAX_TRI2D_TEXTURES, MAX_TRI2D_TEXTURE_BYTES, MAX_TRI2D_TEXTURE_SIZE,
    MAX_TRI2D_VERTICES, TRI2D_HEADER_BYTES, TRI2D_MAGIC, TRI2D_VERSION,
};
pub use ui::{
    encode_text_input, focus_record, ime_state_record, wheel_record, TextInputKind,
    UiSemanticAction, UiSemanticNode, UiSemanticRole, UiSemanticSnapshot, UiSemanticsFrame,
    INPUT_FOCUS, INPUT_IME_COMMIT, INPUT_IME_DISABLED, INPUT_IME_ENABLED, INPUT_IME_PREEDIT,
    INPUT_TEXT_COMMIT, INPUT_WHEEL, MAX_UI_SEMANTICS_BYTES, MAX_UI_SEMANTIC_NODES,
    MAX_UI_SEMANTIC_STRING_BYTES, MAX_UI_TEXT_BYTES,
};

pub const ABI_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentationProfile {
    Framebuffer,
    Tri2d,
    WebGpuRaster,
    WebGpu,
}

impl PresentationProfile {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "framebuffer" => Ok(Self::Framebuffer),
            "tri2d" => Ok(Self::Tri2d),
            "webgpu-raster" => Ok(Self::WebGpuRaster),
            "webgpu" => Ok(Self::WebGpu),
            _ => Err(anyhow!("unsupported presentation profile {value}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Framebuffer => "framebuffer",
            Self::Tri2d => "tri2d",
            Self::WebGpuRaster => "webgpu-raster",
            Self::WebGpu => "webgpu",
        }
    }

    pub(crate) fn supports_gpu(self) -> bool {
        matches!(self, Self::WebGpuRaster | Self::WebGpu)
    }

    pub(crate) fn supports_compute(self) -> bool {
        matches!(self, Self::WebGpu)
    }
}
pub const BYTES_PER_PIXEL: usize = 4;
pub const MAX_PROGRAM_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_GUEST_READ: usize = MAX_FRAME_BYTES;
pub const MAX_GUEST_RW_DATA_BYTES: u32 = 64 * 1024 * 1024;
pub const MAX_GUEST_STACK_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_GUEST_HEAP_BYTES: u32 = 128 * 1024 * 1024;
pub const MAX_ASSET_FILES: usize = 2_048;
pub const MAX_ASSET_FILE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_ASSET_BYTES: usize = 256 * 1024 * 1024;
pub const INPUT_EVENT_BYTES: usize = 8;
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u32 = 2;
pub const MAX_AUDIO_SAMPLES_PER_CALL: usize = AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS as usize;
const MAX_ASSET_NAME_BYTES: usize = 1_024;
const MAX_ASSET_READ_BYTES: usize = 16 * 1024 * 1024;
const MAX_HOSTCALL_BYTES_PER_TICK: usize = 32 * 1024 * 1024;
const MAX_HOSTCALLS_PER_INIT: u32 = 131_072;
const MAX_HOSTCALLS_PER_UPDATE: u32 = 65_536;
const MAX_SLEEP_MS_PER_INIT: u32 = 100;
const MAX_SLEEP_MS_PER_UPDATE: u32 = 50;
const MAX_QUEUED_AUDIO_SAMPLES: usize = AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS as usize * 2;
const MAX_QUEUED_INPUT_EVENTS: usize = 4_096;
const MAX_SAVE_BYTES: usize = 1024 * 1024;
const MAX_LOG_BYTES: usize = 4 * 1024;
const MAX_QUEUED_LOGS: usize = 64;
const MAX_QUEUED_GPU_BATCHES: usize = 4;
const MAX_QUEUED_GPU_EVENTS: usize = 256;
const MAX_GPU_SUBMITS_PER_TICK: u32 = 8;
const MAX_GPU_UPLOAD_BYTES_PER_TICK: usize = 16 * 1024 * 1024;
pub const MAX_TRUAPI_FRAME_BYTES: usize = 1024 * 1024;
const MAX_QUEUED_TRUAPI_FRAMES: usize = 32;
const MAX_QUEUED_TRUAPI_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn validate_program_configuration(
    program_len: usize,
    max_gas_per_update: u64,
) -> Result<()> {
    if program_len == 0 || program_len > MAX_PROGRAM_BYTES {
        bail!("guest program must contain 1..={MAX_PROGRAM_BYTES} bytes");
    }
    if max_gas_per_update == 0 {
        bail!("guest gas budget must be nonzero");
    }
    Ok(())
}

pub(crate) fn validate_asset_count(count: usize) -> Result<()> {
    if count > MAX_ASSET_FILES {
        bail!("guest declares {count} assets; maximum is {MAX_ASSET_FILES}");
    }
    Ok(())
}

fn validate_assets<'a>(
    count: usize,
    assets: impl IntoIterator<Item = (&'a str, usize)>,
) -> Result<()> {
    validate_asset_count(count)?;
    let mut total = 0usize;
    for (path, length) in assets {
        if path.len() > MAX_ASSET_NAME_BYTES {
            bail!("guest asset path exceeds {MAX_ASSET_NAME_BYTES} bytes");
        }
        manifest::validate_path(path)?;
        if length > MAX_ASSET_FILE_BYTES {
            bail!("guest asset {path} exceeds {MAX_ASSET_FILE_BYTES} bytes");
        }
        total = total
            .checked_add(length)
            .ok_or_else(|| anyhow!("guest asset byte count overflow"))?;
        if total > MAX_ASSET_BYTES {
            bail!("guest assets exceed {MAX_ASSET_BYTES} bytes");
        }
    }
    Ok(())
}

pub(crate) fn validate_launch_inputs(
    program: &[u8],
    assets: &HashMap<String, Vec<u8>>,
    max_gas_per_update: u64,
) -> Result<()> {
    validate_program_configuration(program.len(), max_gas_per_update)?;
    validate_assets(
        assets.len(),
        assets
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.len())),
    )
}

pub(crate) fn validate_blob(blob: &ProgramBlob) -> Result<()> {
    if blob.rw_data_size() > MAX_GUEST_RW_DATA_BYTES {
        bail!("guest read-write data exceeds {MAX_GUEST_RW_DATA_BYTES} bytes");
    }
    if blob.stack_size() > MAX_GUEST_STACK_BYTES {
        bail!("guest stack exceeds {MAX_GUEST_STACK_BYTES} bytes");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum InputEventType {
    KeyDown = 1,
    KeyUp = 2,
    ButtonDown = 3,
    ButtonUp = 4,
    PointerMove = 5,
    PointerDelta = 6,
    SurfaceMetrics = 7,
}

/// Import through which a guest arms or releases Host pointer capture.
pub const POINTER_CAPTURE_IMPORT: &str = "host_pointer_capture";
/// The guest asks the Host to release capture and stop arming it.
pub const POINTER_CAPTURE_RELEASE: u32 = 0;
/// The guest arms capture for the next eligible primary activation.
pub const POINTER_CAPTURE_ARM: u32 = 1;
/// Capture is neither armed nor active.
pub const POINTER_CAPTURE_RELEASED: i32 = 0;
/// Capture is armed and waits for the next eligible primary activation.
pub const POINTER_CAPTURE_ARMED: i32 = 1;
/// The Host currently captures the pointer.
pub const POINTER_CAPTURE_ACTIVE: i32 = 2;
/// The Host has no pointer-capture policy on this platform.
pub const POINTER_CAPTURE_UNSUPPORTED: i32 = -1;
/// The request value is not a defined pointer-capture request.
pub const POINTER_CAPTURE_INVALID_REQUEST: i32 = -2;

/// Host pointer-capture policy shared with the guest.
///
/// ABI v1 keeps capture as Host policy: the guest may arm it, the Host decides
/// when an activation is eligible, and the user can always end it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct PointerCaptureState {
    supported: bool,
    armed: bool,
    active: bool,
    request: Option<bool>,
}

impl PointerCaptureState {
    fn status(&self) -> i32 {
        if self.active {
            POINTER_CAPTURE_ACTIVE
        } else if self.armed {
            POINTER_CAPTURE_ARMED
        } else {
            POINTER_CAPTURE_RELEASED
        }
    }

    fn request(&mut self, value: u32) -> i32 {
        if !self.supported {
            return POINTER_CAPTURE_UNSUPPORTED;
        }
        match value {
            POINTER_CAPTURE_ARM => {
                self.armed = true;
                self.request = Some(true);
            }
            POINTER_CAPTURE_RELEASE => {
                self.armed = false;
                self.request = Some(false);
            }
            _ => return POINTER_CAPTURE_INVALID_REQUEST,
        }
        self.status()
    }

    /// Records the capture state the Host actually reached, returning true when
    /// the guest must be told about the transition.
    fn set_active(&mut self, active: bool) -> bool {
        if self.active == active {
            return false;
        }
        self.active = active;
        if active {
            self.armed = false;
        }
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputEvent {
    pub event_type: InputEventType,
    pub code: u8,
    pub x: u16,
    pub y: u16,
}

impl InputEvent {
    fn encode(self) -> [u8; INPUT_EVENT_BYTES] {
        let x = self.x.to_le_bytes();
        let y = self.y.to_le_bytes();
        [
            self.event_type as u8,
            self.code,
            x[0],
            x[1],
            y[0],
            y[1],
            0,
            0,
        ]
    }
}

#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Packed 0xAARRGGBB pixels. On little-endian guests these bytes are BGRA.
    pub argb: Vec<u8>,
}

#[derive(Debug)]
pub struct AudioChunk {
    /// Interleaved little-endian signed 16-bit samples.
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u32,
}

#[derive(Debug)]
pub struct GpuBatch {
    pub bytes: Vec<u8>,
}

/// One validated UI platform-output stream emitted by the guest.
#[derive(Debug)]
pub struct UiOutputFrame {
    /// Canonical [`ui_wire`] bytes.
    pub bytes: Vec<u8>,
}

struct HostClock {
    #[cfg(not(target_arch = "wasm32"))]
    started: Instant,
    #[cfg(target_arch = "wasm32")]
    now_ms: u64,
}

impl HostClock {
    fn new() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            started: Instant::now(),
            #[cfg(target_arch = "wasm32")]
            now_ms: 0,
        }
    }

    fn elapsed_ms(&self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.started.elapsed().as_millis() as u64
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.now_ms
        }
    }

    fn sleep_ms(&mut self, duration_ms: u32) {
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(Duration::from_millis(duration_ms.into()));
        #[cfg(target_arch = "wasm32")]
        {
            self.now_ms = self.now_ms.saturating_add(duration_ms.into());
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn set_time_ms(&mut self, time_ms: u64) {
        self.now_ms = self.now_ms.max(time_ms);
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MotionState {
    availability: motion_wire::MotionAvailability,
    sample: Option<[u8; motion_wire::MOTION_SAMPLE_BYTES]>,
}

impl MotionState {
    fn new() -> Self {
        Self {
            availability: motion_wire::MotionAvailability::Unavailable,
            sample: None,
        }
    }

    pub(crate) fn set_availability(&mut self, availability: motion_wire::MotionAvailability) {
        self.availability = availability;
        if availability != motion_wire::MotionAvailability::Available {
            self.sample = None;
        }
    }

    pub(crate) fn set_sample(&mut self, bytes: &[u8]) -> Result<()> {
        motion_wire::MotionSample::decode(bytes)
            .map_err(|error| anyhow!("invalid motion sample: {error}"))?;
        let sample: [u8; motion_wire::MOTION_SAMPLE_BYTES] = bytes
            .try_into()
            .map_err(|_| anyhow!("invalid motion sample length"))?;
        self.availability = motion_wire::MotionAvailability::Available;
        self.sample = Some(sample);
        Ok(())
    }

    pub(crate) fn read(
        &self,
        capacity: usize,
    ) -> core::result::Result<Option<[u8; motion_wire::MOTION_SAMPLE_BYTES]>, i32> {
        match self.availability {
            motion_wire::MotionAvailability::Unavailable => {
                Err(motion_wire::MOTION_ERROR_UNAVAILABLE)
            }
            motion_wire::MotionAvailability::PermissionDenied => {
                Err(motion_wire::MOTION_ERROR_PERMISSION_DENIED)
            }
            motion_wire::MotionAvailability::Available => {
                if capacity < motion_wire::MOTION_SAMPLE_BYTES {
                    Err(motion_wire::MOTION_ERROR_BUFFER_TOO_SMALL)
                } else {
                    Ok(self.sample)
                }
            }
        }
    }

    pub(crate) fn consume(&mut self) {
        self.sample = None;
    }
}

pub(crate) fn preferred_backend() -> BackendKind {
    #[cfg(any(target_arch = "wasm32", target_os = "ios", target_os = "android"))]
    {
        BackendKind::Interpreter
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "ios", target_os = "android")))]
    {
        BackendKind::Compiler
    }
}

struct HostState {
    frame: Option<Frame>,
    tri2d: Option<Tri2dFrame>,
    tri2d_state: tri2d::Tri2dState,
    audio_enabled: bool,
    presentation: PresentationProfile,
    tri2d_submitted: bool,
    ui_semantics: Option<UiSemanticsFrame>,
    ui_semantics_submitted: bool,
    ui_output: Option<UiOutputFrame>,
    ui_output_submitted: bool,
    audio: VecDeque<AudioChunk>,
    audio_samples: usize,
    input: VecDeque<[u8; INPUT_EVENT_BYTES]>,
    assets: HashMap<String, Vec<u8>>,
    clock: HostClock,
    logs: VecDeque<String>,
    save: Option<Vec<u8>>,
    hostcall_bytes_remaining: usize,
    hostcalls_remaining: u32,
    sleep_ms_remaining: u32,
    motion: MotionState,
    pointer_capture: PointerCaptureState,
    uses_pointer_capture: bool,
    uses_motion: bool,
    gpu_capabilities: Option<Vec<u8>>,
    gpu_batches: VecDeque<GpuBatch>,
    gpu_events: VecDeque<Vec<u8>>,
    truapi_requests: VecDeque<Vec<u8>>,
    truapi_request_bytes: usize,
    truapi_responses: VecDeque<Vec<u8>>,
    truapi_response_bytes: usize,
    gpu_last_sequence: u64,
    gpu_submits_remaining: u32,
    gpu_upload_bytes_remaining: usize,
}

impl HostState {
    fn new(
        assets: HashMap<String, Vec<u8>>,
        presentation: PresentationProfile,
        audio_enabled: bool,
        uses_motion: bool,
        uses_pointer_capture: bool,
    ) -> Self {
        Self {
            frame: None,
            tri2d: None,
            presentation,
            audio_enabled,
            tri2d_state: tri2d::Tri2dState::default(),
            tri2d_submitted: false,
            ui_semantics: None,
            ui_semantics_submitted: false,
            ui_output: None,
            ui_output_submitted: false,
            audio: VecDeque::new(),
            audio_samples: 0,
            input: VecDeque::new(),
            assets,
            clock: HostClock::new(),
            logs: VecDeque::new(),
            save: None,
            hostcall_bytes_remaining: 0,
            hostcalls_remaining: 0,
            sleep_ms_remaining: 0,
            motion: MotionState::new(),
            pointer_capture: PointerCaptureState::default(),
            uses_pointer_capture,
            uses_motion,
            gpu_capabilities: None,
            gpu_batches: VecDeque::new(),
            gpu_events: VecDeque::new(),
            truapi_requests: VecDeque::new(),
            truapi_request_bytes: 0,
            truapi_responses: VecDeque::new(),
            truapi_response_bytes: 0,
            gpu_last_sequence: 0,
            gpu_submits_remaining: 0,
            gpu_upload_bytes_remaining: 0,
        }
    }

    fn reset_hostcall_budget(&mut self, max_hostcalls: u32, max_sleep_ms: u32) {
        self.hostcall_bytes_remaining = MAX_HOSTCALL_BYTES_PER_TICK;
        self.hostcalls_remaining = max_hostcalls;
        self.sleep_ms_remaining = max_sleep_ms;
        self.tri2d_submitted = false;
        self.ui_semantics_submitted = false;
        self.ui_output_submitted = false;
        self.gpu_submits_remaining = MAX_GPU_SUBMITS_PER_TICK;
        self.gpu_upload_bytes_remaining = MAX_GPU_UPLOAD_BYTES_PER_TICK;
    }

    fn charge_hostcall(&mut self, bytes: usize) -> Result<()> {
        if self.hostcalls_remaining == 0 {
            return Err(anyhow!("guest exceeded host-call count budget"));
        }
        self.hostcalls_remaining -= 1;
        self.charge_hostcall_bytes(bytes)
    }

    fn charge_hostcall_bytes(&mut self, bytes: usize) -> Result<()> {
        if bytes > self.hostcall_bytes_remaining {
            return Err(anyhow!("guest exceeded host-call byte budget"));
        }
        self.hostcall_bytes_remaining -= bytes;
        Ok(())
    }

    fn queue_input(&mut self, event: InputEvent) {
        let _ = self.queue_input_record(event.encode());
    }

    fn queue_input_record(&mut self, record: [u8; INPUT_EVENT_BYTES]) -> Result<()> {
        ui::validate_input_record(&record)?;
        if record[0] == InputEventType::SurfaceMetrics as u8 {
            if let Some(position) = self
                .input
                .iter()
                .rposition(|queued| queued[0] == InputEventType::SurfaceMetrics as u8)
            {
                self.input.remove(position);
            }
        } else if record[0] == InputEventType::PointerMove as u8
            && self
                .input
                .back()
                .is_some_and(|queued| queued[0] == InputEventType::PointerMove as u8)
        {
            self.input.pop_back();
        }
        if self.input.len() == MAX_QUEUED_INPUT_EVENTS {
            let discardable = self
                .input
                .iter()
                .position(|queued| queued[0] == InputEventType::PointerMove as u8)
                .or_else(|| {
                    self.input.iter().position(|queued| {
                        matches!(
                            queued[0],
                            value if value == InputEventType::PointerDelta as u8
                                || value == ui::INPUT_WHEEL
                        )
                    })
                });
            if let Some(position) = discardable {
                self.input.remove(position);
            } else {
                bail!("input queue is full");
            }
        }
        self.input.push_back(record);
        Ok(())
    }

    fn queue_input_records(&mut self, records: Vec<[u8; INPUT_EVENT_BYTES]>) -> Result<()> {
        if self.input.len().saturating_add(records.len()) > MAX_QUEUED_INPUT_EVENTS {
            bail!("input queue cannot accept a complete text event");
        }
        for record in &records {
            ui::validate_input_record(record)?;
        }
        self.input.extend(records);
        Ok(())
    }

    fn queue_ui_semantics(&mut self, bytes: Vec<u8>) -> Result<()> {
        if self.ui_semantics_submitted {
            bail!("UI semantics were already submitted during this call");
        }
        ui::validate_ui_semantics(&bytes)?;
        self.ui_semantics = Some(UiSemanticsFrame { bytes });
        self.ui_semantics_submitted = true;
        Ok(())
    }

    fn queue_ui_output(&mut self, bytes: Vec<u8>) -> Result<()> {
        if self.ui_output_submitted {
            bail!("UI output was already submitted during this call");
        }
        ui_wire::decode_ui_output(&bytes).map_err(|error| anyhow!("invalid UI output: {error}"))?;
        self.ui_output = Some(UiOutputFrame { bytes });
        self.ui_output_submitted = true;
        Ok(())
    }

    fn take_truapi_request(&mut self) -> Option<Vec<u8>> {
        let frame = self.truapi_requests.pop_front()?;
        self.truapi_request_bytes -= frame.len();
        Some(frame)
    }

    fn queue_truapi_response(&mut self, bytes: Vec<u8>) -> Result<()> {
        if bytes.is_empty() || bytes.len() > MAX_TRUAPI_FRAME_BYTES {
            return Err(anyhow!("invalid TrUAPI response frame"));
        }
        if self.truapi_responses.len() == MAX_QUEUED_TRUAPI_FRAMES
            || self.truapi_response_bytes.saturating_add(bytes.len()) > MAX_QUEUED_TRUAPI_BYTES
        {
            return Err(anyhow!("TrUAPI response queue overflow"));
        }
        self.truapi_response_bytes += bytes.len();
        self.truapi_responses.push_back(bytes);
        Ok(())
    }
}

pub struct Runtime {
    instance: Instance<HostState, anyhow::Error>,
    state: HostState,
    max_gas_per_update: u64,
    last_gas_used: u64,
    backend: polkavm::BackendKind,
}

impl Runtime {
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
            preferred_backend(),
        )
    }

    pub fn new_with_backend(
        program: &[u8],
        assets: HashMap<String, Vec<u8>>,
        presentation: PresentationProfile,
        audio_enabled: bool,
        max_gas_per_update: u64,
        backend: BackendKind,
    ) -> Result<Self> {
        validate_launch_inputs(program, &assets, max_gas_per_update)?;
        let blob = ProgramBlob::parse(program.into()).context("parse PolkaVM program")?;
        validate_blob(&blob)?;
        Self::from_blob(
            blob,
            assets,
            presentation,
            audio_enabled,
            max_gas_per_update,
            backend,
        )
    }

    pub(crate) fn from_blob(
        blob: ProgramBlob,
        assets: HashMap<String, Vec<u8>>,
        presentation: PresentationProfile,
        audio_enabled: bool,
        max_gas_per_update: u64,
        backend: BackendKind,
    ) -> Result<Self> {
        let imports = blob.imports();
        let uses_motion = imports
            .iter()
            .flatten()
            .any(|import| import.as_bytes() == motion_wire::MOTION_READ_IMPORT.as_bytes());
        let uses_pointer_capture = imports
            .iter()
            .flatten()
            .any(|import| import.as_bytes() == POINTER_CAPTURE_IMPORT.as_bytes());
        let mut engine_config = Config::new();
        // macOS requires PolkaVM's experimental generic sandbox for native
        // recompilation. Keep sandboxing enabled while opting into that boundary.
        engine_config.set_backend(Some(backend));
        engine_config.set_sandboxing_enabled(true);
        #[cfg(target_os = "macos")]
        {
            engine_config.set_sandbox(Some(polkavm::SandboxKind::Generic));
            engine_config.set_allow_experimental(true);
        }
        let engine = Engine::new(&engine_config).context("create PolkaVM engine")?;
        let backend = engine.backend();

        let mut module_config = polkavm::ModuleConfig::default();
        module_config.set_gas_metering(Some(polkavm::GasMeteringKind::Sync));
        #[cfg(not(target_arch = "wasm32"))]
        module_config.set_max_heap_size(Some(MAX_GUEST_HEAP_BYTES));
        #[cfg(target_os = "macos")]
        module_config.set_page_size(16_384);

        let module =
            Module::from_blob(&engine, &module_config, blob).context("compile PolkaVM module")?;
        let mut linker: Linker<HostState, anyhow::Error> = Linker::new();

        linker
            .define_typed(
                "host_present_frame",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 width: u32,
                 height: u32,
                 stride: u32|
                 -> Result<u32> {
                    let Some(row_bytes) = width.checked_mul(BYTES_PER_PIXEL as u32) else {
                        return Ok(1);
                    };
                    if stride != row_bytes {
                        return Ok(1);
                    }
                    let Some(byte_len) = row_bytes.checked_mul(height) else {
                        return Ok(1);
                    };
                    let byte_len = byte_len as usize;
                    if byte_len == 0 || byte_len > MAX_FRAME_BYTES {
                        return Ok(1);
                    }
                    caller.user_data.charge_hostcall(byte_len)?;
                    if caller.user_data.presentation != PresentationProfile::Framebuffer {
                        return Ok(3);
                    }
                    let argb = read_guest_memory(caller.instance, pointer, byte_len)?;
                    caller.user_data.frame = Some(Frame {
                        width,
                        height,
                        argb,
                    });
                    Ok(0)
                },
            )
            .context("define host_present_frame")?;

        linker
            .define_typed(
                "host_tri2d_submit",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 length: u32|
                 -> Result<u32> {
                    let length = length as usize;
                    if length == 0 || length > MAX_TRI2D_BYTES {
                        return Ok(1);
                    }
                    caller.user_data.charge_hostcall(length)?;
                    if caller.user_data.presentation != PresentationProfile::Tri2d {
                        return Ok(3);
                    }
                    if caller.user_data.tri2d_submitted {
                        return Ok(2);
                    }
                    let bytes = read_guest_memory(caller.instance, pointer, length)?;
                    let Ok((next_state, metadata)) =
                        tri2d::validate_tri2d(&bytes, &caller.user_data.tri2d_state)
                    else {
                        return Ok(1);
                    };
                    caller.user_data.tri2d_state = next_state;
                    caller.user_data.tri2d_submitted = true;
                    caller.user_data.tri2d = Some(Tri2dFrame {
                        width: metadata.width,
                        height: metadata.height,
                        draw_count: metadata.draw_count,
                        vertex_count: metadata.vertex_count,
                        index_count: metadata.index_count,
                        bytes,
                    });
                    Ok(0)
                },
            )
            .context("define host_tri2d_submit")?;

        linker
            .define_typed(
                "host_ui_semantics_submit",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 length: u32|
                 -> Result<u32> {
                    let length = length as usize;
                    if length == 0 || length > MAX_UI_SEMANTICS_BYTES {
                        return Ok(1);
                    }
                    caller.user_data.charge_hostcall(length)?;
                    if caller.user_data.ui_semantics_submitted {
                        return Ok(2);
                    }
                    let bytes = read_guest_memory(caller.instance, pointer, length)?;
                    if caller.user_data.queue_ui_semantics(bytes).is_err() {
                        return Ok(1);
                    }
                    Ok(0)
                },
            )
            .context("define host_ui_semantics_submit")?;

        linker
            .define_typed(
                ui_wire::UI_OUTPUT_SUBMIT_IMPORT,
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 length: u32|
                 -> Result<u32> {
                    let length = length as usize;
                    if !(ui_wire::UI_OUTPUT_HEADER_BYTES..=ui_wire::MAX_UI_OUTPUT_BYTES)
                        .contains(&length)
                    {
                        return Ok(ui_wire::UI_OUTPUT_SUBMIT_INVALID);
                    }
                    caller.user_data.charge_hostcall(length)?;
                    if caller.user_data.ui_output_submitted {
                        return Ok(ui_wire::UI_OUTPUT_SUBMIT_DUPLICATE);
                    }
                    let Ok(bytes) = read_guest_memory(caller.instance, pointer, length) else {
                        return Ok(ui_wire::UI_OUTPUT_SUBMIT_INVALID);
                    };
                    if caller.user_data.queue_ui_output(bytes).is_err() {
                        return Ok(ui_wire::UI_OUTPUT_SUBMIT_INVALID);
                    }
                    Ok(ui_wire::UI_OUTPUT_SUBMIT_ACCEPTED)
                },
            )
            .context("define host_ui_output_submit")?;

        linker
            .define_typed(
                "host_gpu_capabilities",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 capacity: u32|
                 -> Result<i32> {
                    caller.user_data.charge_hostcall(0)?;
                    if !caller.user_data.presentation.supports_gpu() {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_STATE);
                    }
                    let Some(required_len) =
                        caller.user_data.gpu_capabilities.as_ref().map(Vec::len)
                    else {
                        return Ok(0);
                    };
                    let required = i32::try_from(required_len)
                        .map_err(|_| anyhow!("GPU capabilities length overflow"))?;
                    if (capacity as usize) < required_len {
                        return Ok(-required);
                    }
                    caller.user_data.charge_hostcall_bytes(required_len)?;
                    let capabilities = caller.user_data.gpu_capabilities.as_ref().unwrap();
                    if caller.instance.write_memory(pointer, capabilities).is_err() {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_GUEST_RANGE);
                    }
                    Ok(required)
                },
            )
            .context("define host_gpu_capabilities")?;

        linker
            .define_typed(
                "host_gpu_submit",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 length: u32|
                 -> Result<i32> {
                    let length = length as usize;
                    caller.user_data.charge_hostcall(0)?;
                    if !caller.user_data.presentation.supports_gpu() {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_STATE);
                    }
                    if length == 0 || length > gpu_wire::MAX_GPU_BATCH_BYTES {
                        return Ok(gpu_wire::GPU_ERROR_MALFORMED_BATCH);
                    }
                    caller.user_data.charge_hostcall_bytes(length)?;
                    if caller.user_data.gpu_capabilities.is_none() {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_STATE);
                    }
                    if caller.user_data.gpu_batches.len() == MAX_QUEUED_GPU_BATCHES {
                        return Ok(gpu_wire::GPU_SUBMIT_BUSY);
                    }
                    if caller.user_data.gpu_submits_remaining == 0 {
                        return Ok(gpu_wire::GPU_ERROR_QUOTA_EXCEEDED);
                    }
                    let Ok(bytes) = read_guest_memory(caller.instance, pointer, length) else {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_GUEST_RANGE);
                    };
                    let Ok(batch) = gpu_wire::decode_gpu_batch(&bytes) else {
                        return Ok(gpu_wire::GPU_ERROR_MALFORMED_BATCH);
                    };
                    if !gpu_batch_supported(caller.user_data.presentation, &batch) {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_STATE);
                    }
                    if batch.sequence() <= caller.user_data.gpu_last_sequence {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_STATE);
                    }
                    let Some(upload_bytes) = gpu_inline_upload_bytes(&batch) else {
                        return Ok(gpu_wire::GPU_ERROR_MALFORMED_BATCH);
                    };
                    if upload_bytes > caller.user_data.gpu_upload_bytes_remaining {
                        return Ok(gpu_wire::GPU_ERROR_QUOTA_EXCEEDED);
                    }
                    caller.user_data.gpu_submits_remaining -= 1;
                    caller.user_data.gpu_upload_bytes_remaining -= upload_bytes;
                    caller.user_data.gpu_last_sequence = batch.sequence();
                    caller.user_data.gpu_batches.push_back(GpuBatch { bytes });
                    Ok(gpu_wire::GPU_SUBMIT_ACCEPTED)
                },
            )
            .context("define host_gpu_submit")?;

        linker
            .define_typed(
                "host_gpu_receive",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 capacity: u32|
                 -> Result<i32> {
                    caller.user_data.charge_hostcall(0)?;
                    if !caller.user_data.presentation.supports_gpu() {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_STATE);
                    }
                    let Some(required_len) = caller.user_data.gpu_events.front().map(Vec::len)
                    else {
                        return Ok(0);
                    };
                    let required = i32::try_from(required_len)
                        .map_err(|_| anyhow!("GPU event length overflow"))?;
                    if (capacity as usize) < required_len {
                        return Ok(-required);
                    }
                    caller.user_data.charge_hostcall_bytes(required_len)?;
                    let event = caller.user_data.gpu_events.front().unwrap();
                    if caller.instance.write_memory(pointer, event).is_err() {
                        return Ok(gpu_wire::GPU_ERROR_INVALID_GUEST_RANGE);
                    }
                    caller.user_data.gpu_events.pop_front();
                    Ok(required)
                },
            )
            .context("define host_gpu_receive")?;

        linker
            .define_typed(
                "host_truapi_send",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 length: u32|
                 -> Result<u32> {
                    let length = length as usize;
                    if length == 0 || length > MAX_TRUAPI_FRAME_BYTES {
                        return Ok(1);
                    }
                    caller.user_data.charge_hostcall(length)?;
                    if caller.user_data.truapi_requests.len() == MAX_QUEUED_TRUAPI_FRAMES
                        || caller.user_data.truapi_request_bytes.saturating_add(length)
                            > MAX_QUEUED_TRUAPI_BYTES
                    {
                        return Ok(2);
                    }
                    let bytes = read_guest_memory(caller.instance, pointer, length)?;
                    caller.user_data.truapi_request_bytes += bytes.len();
                    caller.user_data.truapi_requests.push_back(bytes);
                    Ok(0)
                },
            )
            .context("define host_truapi_send")?;

        linker
            .define_typed(
                "host_truapi_poll",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 capacity: u32|
                 -> Result<i32> {
                    caller.user_data.charge_hostcall(0)?;
                    let Some(required_len) =
                        caller.user_data.truapi_responses.front().map(Vec::len)
                    else {
                        return Ok(0);
                    };
                    let required = i32::try_from(required_len)
                        .map_err(|_| anyhow!("TrUAPI response length overflow"))?;
                    if (capacity as usize) < required_len {
                        return Ok(-required);
                    }
                    caller.user_data.charge_hostcall_bytes(required_len)?;
                    let response = caller.user_data.truapi_responses.front().unwrap();
                    caller
                        .instance
                        .write_memory(pointer, response)
                        .map_err(|error| anyhow!("write TrUAPI response: {error:?}"))?;
                    caller.user_data.truapi_responses.pop_front();
                    caller.user_data.truapi_response_bytes -= required_len;
                    Ok(required)
                },
            )
            .context("define host_truapi_poll")?;

        linker
            .define_typed(
                "host_poll_input",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 capacity: u32|
                 -> Result<u32> {
                    let event_count =
                        (capacity as usize / INPUT_EVENT_BYTES).min(caller.user_data.input.len());
                    let byte_count = event_count
                        .checked_mul(INPUT_EVENT_BYTES)
                        .ok_or_else(|| anyhow!("input byte count overflow"))?;
                    caller.user_data.charge_hostcall(byte_count)?;
                    let mut written = 0u32;
                    for _ in 0..event_count {
                        let Some(event) = caller.user_data.input.pop_front() else {
                            break;
                        };
                        let destination = pointer
                            .checked_add(written)
                            .ok_or_else(|| anyhow!("guest input destination overflow"))?;
                        caller
                            .instance
                            .write_memory(destination, &event)
                            .map_err(|error| anyhow!("write guest input: {error:?}"))?;
                        written += INPUT_EVENT_BYTES as u32;
                    }
                    Ok(written)
                },
            )
            .context("define host_poll_input")?;

        linker
            .define_typed(
                motion_wire::MOTION_READ_IMPORT,
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 capacity: u32|
                 -> Result<i32> {
                    caller.user_data.charge_hostcall(0)?;
                    let sample = match caller.user_data.motion.read(capacity as usize) {
                        Ok(Some(sample)) => sample,
                        Ok(None) => return Ok(motion_wire::MOTION_READ_NO_SAMPLE),
                        Err(status) => return Ok(status),
                    };
                    caller
                        .user_data
                        .charge_hostcall_bytes(motion_wire::MOTION_SAMPLE_BYTES)?;
                    if caller.instance.write_memory(pointer, &sample).is_err() {
                        return Ok(motion_wire::MOTION_ERROR_INVALID_GUEST_RANGE);
                    }
                    caller.user_data.motion.consume();
                    Ok(motion_wire::MOTION_SAMPLE_BYTES as i32)
                },
            )
            .context("define host_motion_read")?;

        linker
            .define_typed(
                POINTER_CAPTURE_IMPORT,
                |caller: polkavm::Caller<'_, HostState>, request: u32| -> Result<i32> {
                    caller.user_data.charge_hostcall(0)?;
                    Ok(caller.user_data.pointer_capture.request(request))
                },
            )
            .context("define host_pointer_capture")?;

        linker
            .define_typed(
                "host_time_ms",
                |caller: polkavm::Caller<'_, HostState>| -> Result<u64> {
                    caller.user_data.charge_hostcall(0)?;
                    Ok(caller.user_data.clock.elapsed_ms())
                },
            )
            .context("define host_time_ms")?;

        linker
            .define_typed(
                "host_sleep_ms",
                |caller: polkavm::Caller<'_, HostState>, duration_ms: u32| -> Result<()> {
                    caller.user_data.charge_hostcall(0)?;
                    let duration_ms = duration_ms.min(caller.user_data.sleep_ms_remaining);
                    caller.user_data.sleep_ms_remaining -= duration_ms;
                    caller.user_data.clock.sleep_ms(duration_ms);
                    Ok(())
                },
            )
            .context("define host_sleep_ms")?;

        linker
            .define_typed(
                "host_audio_submit",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 sample_count: u32|
                 -> Result<u32> {
                    let sample_count = sample_count as usize;
                    if sample_count == 0
                        || !sample_count.is_multiple_of(AUDIO_CHANNELS as usize)
                        || sample_count > MAX_AUDIO_SAMPLES_PER_CALL
                        || caller.user_data.audio_samples + sample_count > MAX_QUEUED_AUDIO_SAMPLES
                    {
                        return Ok(1);
                    }
                    let byte_count = sample_count
                        .checked_mul(size_of::<i16>())
                        .ok_or_else(|| anyhow!("audio byte count overflow"))?;
                    caller.user_data.charge_hostcall(byte_count)?;
                    if !caller.user_data.audio_enabled {
                        return Ok(3);
                    }
                    let bytes = read_guest_memory(caller.instance, pointer, byte_count)?;
                    let samples = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|sample| i16::from_le_bytes(*sample))
                        .collect();
                    caller.user_data.audio_samples += sample_count;
                    caller.user_data.audio.push_back(AudioChunk {
                        samples,
                        sample_rate: AUDIO_SAMPLE_RATE,
                        channels: AUDIO_CHANNELS,
                    });
                    Ok(0)
                },
            )
            .context("define host_audio_submit")?;

        linker
            .define_typed(
                "host_asset_read",
                |caller: polkavm::Caller<'_, HostState>,
                 name_pointer: u32,
                 name_length: u32,
                 offset: u32,
                 destination: u32,
                 capacity: u32|
                 -> Result<u32> {
                    let name_length = name_length as usize;
                    caller.user_data.charge_hostcall(0)?;
                    if name_length == 0 || name_length > MAX_ASSET_NAME_BYTES {
                        return Ok(0);
                    }
                    caller.user_data.charge_hostcall_bytes(name_length)?;
                    let name = read_guest_memory(caller.instance, name_pointer, name_length)?;
                    let Ok(name) = std::str::from_utf8(&name) else {
                        return Ok(0);
                    };
                    let Some(asset_length) = caller.user_data.assets.get(name).map(Vec::len) else {
                        return Ok(0);
                    };
                    let offset = offset as usize;
                    if offset >= asset_length {
                        return Ok(0);
                    }
                    let length = (capacity as usize)
                        .min(asset_length - offset)
                        .min(MAX_ASSET_READ_BYTES);
                    caller.user_data.charge_hostcall_bytes(length)?;
                    let asset = &caller.user_data.assets[name];
                    caller
                        .instance
                        .write_memory(destination, &asset[offset..offset + length])
                        .map_err(|error| anyhow!("write guest asset: {error:?}"))?;
                    Ok(length as u32)
                },
            )
            .context("define host_asset_read")?;

        linker
            .define_typed(
                "host_save_submit",
                |caller: polkavm::Caller<'_, HostState>,
                 pointer: u32,
                 length: u32|
                 -> Result<u32> {
                    let length = length as usize;
                    if length == 0 || length > MAX_SAVE_BYTES {
                        return Ok(1);
                    }
                    caller.user_data.charge_hostcall(length)?;
                    caller.user_data.save =
                        Some(read_guest_memory(caller.instance, pointer, length)?);
                    Ok(0)
                },
            )
            .context("define host_save_submit")?;

        linker
            .define_typed(
                "host_log",
                |caller: polkavm::Caller<'_, HostState>, pointer: u32, length: u32| -> Result<()> {
                    let length = (length as usize).min(MAX_LOG_BYTES);
                    caller.user_data.charge_hostcall(length)?;
                    let bytes = read_guest_memory(caller.instance, pointer, length)?;
                    if caller.user_data.logs.len() == MAX_QUEUED_LOGS {
                        caller.user_data.logs.pop_front();
                    }
                    caller
                        .user_data
                        .logs
                        .push_back(String::from_utf8_lossy(&bytes).into_owned());
                    Ok(())
                },
            )
            .context("define host_log")?;

        let instance = linker
            .instantiate_pre(&module)
            .context("pre-instantiate PolkaVM module")?
            .instantiate()
            .context("instantiate PolkaVM module")?;

        Ok(Self {
            instance,
            state: HostState::new(
                assets,
                presentation,
                audio_enabled,
                uses_motion,
                uses_pointer_capture,
            ),
            max_gas_per_update,
            last_gas_used: 0,
            backend,
        })
    }

    pub fn init(&mut self) -> Result<()> {
        let gas = self.max_gas_per_update.min(i64::MAX as u64) as i64;
        self.instance.set_gas(gas);
        self.state
            .reset_hostcall_budget(MAX_HOSTCALLS_PER_INIT, MAX_SLEEP_MS_PER_INIT);
        let result = self
            .instance
            .call_typed_and_get_result::<(), ()>(&mut self.state, "init", ());
        self.record_gas_used(gas);
        let program_counter = self.instance.program_counter();
        map_call_result(result, "init").map_err(|error| {
            let error = if let Some(log) = self.state.logs.back() {
                error.context(format!("last guest log: {log}"))
            } else {
                error
            };
            if let Some(program_counter) = program_counter {
                error.context(format!("guest program counter: {program_counter}"))
            } else {
                error
            }
        })
    }

    pub fn update(&mut self) -> Result<()> {
        let gas = self.max_gas_per_update.min(i64::MAX as u64) as i64;
        self.instance.set_gas(gas);
        self.state
            .reset_hostcall_budget(MAX_HOSTCALLS_PER_UPDATE, MAX_SLEEP_MS_PER_UPDATE);
        let result =
            self.instance
                .call_typed_and_get_result::<(), ()>(&mut self.state, "update", ());
        self.record_gas_used(gas);
        let program_counter = self.instance.program_counter();
        map_call_result(result, "update").map_err(|error| {
            let error = if let Some(log) = self.state.logs.back() {
                error.context(format!("last guest log: {log}"))
            } else {
                error
            };
            if let Some(program_counter) = program_counter {
                error.context(format!("guest program counter: {program_counter}"))
            } else {
                error
            }
        })
    }
    fn record_gas_used(&mut self, budget: i64) {
        let remaining = self.instance.gas().max(0);
        self.last_gas_used = (budget - remaining.min(budget)) as u64;
    }

    pub fn last_gas_used(&self) -> u64 {
        self.last_gas_used
    }

    pub fn backend(&self) -> polkavm::BackendKind {
        self.backend
    }

    pub fn uses_motion(&self) -> bool {
        self.state.uses_motion
    }

    pub fn send_input(&mut self, event: InputEvent) {
        self.state.queue_input(event);
    }

    pub fn send_input_record(&mut self, record: [u8; INPUT_EVENT_BYTES]) -> Result<()> {
        self.state.queue_input_record(record)
    }

    pub fn send_text_input(&mut self, kind: TextInputKind, text: &str) -> Result<()> {
        self.state
            .queue_input_records(ui::encode_text_input(kind, text)?)
    }

    pub fn set_motion_availability(&mut self, availability: motion_wire::MotionAvailability) {
        self.state.motion.set_availability(availability);
    }

    /// True when the guest imports the pointer-capture hostcall.
    pub fn uses_pointer_capture(&self) -> bool {
        self.state.uses_pointer_capture
    }

    /// Declares whether this Host can capture the pointer at all. Revoking
    /// support drops any request the guest has not been served yet, so the Host
    /// never arms capture it has just said it cannot perform.
    pub fn set_pointer_capture_supported(&mut self, supported: bool) {
        self.state.pointer_capture.supported = supported;
        if !supported {
            self.state.pointer_capture.armed = false;
            self.state.pointer_capture.request = None;
        }
    }

    /// Reports the capture state the Host reached, including capture the user
    /// ended, and tells the guest about every transition. The transition is
    /// committed only once the guest has been told, so a rejected record leaves
    /// the Host free to report the same state again.
    pub fn set_pointer_capture_active(&mut self, active: bool) -> Result<()> {
        if self.state.pointer_capture.active == active {
            return Ok(());
        }
        self.state
            .queue_input_record(ui::pointer_capture_record(active))?;
        self.state.pointer_capture.set_active(active);
        Ok(())
    }

    /// Takes the newest guest capture request: `true` arms capture for the next
    /// eligible primary activation, `false` releases it.
    pub fn take_pointer_capture_request(&mut self) -> Option<bool> {
        self.state.pointer_capture.request.take()
    }

    pub fn send_motion_sample(&mut self, bytes: &[u8]) -> Result<()> {
        self.state.motion.set_sample(bytes)
    }

    pub fn gpu_ready(&self) -> bool {
        !self.state.presentation.supports_gpu() || self.state.gpu_capabilities.is_some()
    }

    pub fn set_gpu_capabilities(&mut self, bytes: Vec<u8>) -> Result<()> {
        if !self.state.presentation.supports_gpu() {
            return Err(anyhow!("GPU capabilities sent to a non-GPU application"));
        }
        validate_gpu_capabilities(&bytes)?;
        self.state.gpu_capabilities = Some(bytes);
        Ok(())
    }

    pub fn send_gpu_event(&mut self, bytes: Vec<u8>) -> Result<()> {
        if !self.state.presentation.supports_gpu() {
            return Err(anyhow!("GPU event sent to a non-GPU application"));
        }
        validate_gpu_event(&bytes)?;
        if self.state.gpu_events.len() == MAX_QUEUED_GPU_EVENTS {
            return Err(anyhow!("GPU event queue overflow"));
        }
        self.state.gpu_events.push_back(bytes);
        Ok(())
    }

    pub fn take_gpu_batch(&mut self) -> Option<GpuBatch> {
        self.state.gpu_batches.pop_front()
    }

    pub fn take_truapi_request(&mut self) -> Option<Vec<u8>> {
        self.state.take_truapi_request()
    }

    pub fn send_truapi_response(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.state.queue_truapi_response(bytes)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_time_ms(&mut self, time_ms: u64) {
        self.state.clock.set_time_ms(time_ms);
    }

    pub fn take_frame(&mut self) -> Option<Frame> {
        self.state.frame.take()
    }

    pub fn take_tri2d(&mut self) -> Option<Tri2dFrame> {
        self.state.tri2d.take()
    }

    pub fn take_ui_semantics(&mut self) -> Option<UiSemanticsFrame> {
        self.state.ui_semantics.take()
    }

    /// Take the newest UI platform-output snapshot, if one is pending.
    pub fn take_ui_output(&mut self) -> Option<UiOutputFrame> {
        self.state.ui_output.take()
    }

    pub fn take_audio(&mut self) -> Option<AudioChunk> {
        let chunk = self.state.audio.pop_front()?;
        self.state.audio_samples -= chunk.samples.len();
        Some(chunk)
    }

    pub fn take_log(&mut self) -> Option<String> {
        self.state.logs.pop_front()
    }

    pub fn take_save(&mut self) -> Option<Vec<u8>> {
        self.state.save.take()
    }
}

fn gpu_inline_upload_bytes(batch: &gpu_wire::GpuBatch<'_>) -> Option<usize> {
    batch.commands().try_fold(0usize, |total, command| {
        let bytes = match command.opcode {
            gpu_wire::GpuOpcode::WriteBuffer => gpu_u32(command.payload, 16)? as usize,
            gpu_wire::GpuOpcode::WriteTexture => gpu_u32(command.payload, 40)? as usize,
            _ => 0,
        };
        total.checked_add(bytes)
    })
}

fn gpu_batch_supported(presentation: PresentationProfile, batch: &gpu_wire::GpuBatch<'_>) -> bool {
    presentation.supports_compute()
        || batch.commands().all(|command| {
            !matches!(
                command.opcode,
                gpu_wire::GpuOpcode::CreateComputePipeline
                    | gpu_wire::GpuOpcode::BeginComputePass
                    | gpu_wire::GpuOpcode::SetComputePipeline
                    | gpu_wire::GpuOpcode::SetComputeBindGroup
                    | gpu_wire::GpuOpcode::DispatchWorkgroups
                    | gpu_wire::GpuOpcode::EndComputePass
            )
        })
}

fn validate_gpu_capabilities(bytes: &[u8]) -> Result<()> {
    const HEADER_BYTES: usize = 56;
    const ENTRY_BYTES: usize = 16;
    if bytes.len() < HEADER_BYTES
        || bytes[..4] != gpu_wire::GPU_CAPABILITIES_MAGIC
        || gpu_u16(bytes, 4) != Some(gpu_wire::GPU_WIRE_VERSION)
        || gpu_u16(bytes, 6) != Some(0)
        || gpu_u32(bytes, 8) != Some(bytes.len() as u32)
        || !matches!(gpu_u16(bytes, 12), Some(1..=4))
        || gpu_u16(bytes, 14) != Some(0)
        || gpu_u32(bytes, 16) == Some(0)
        || gpu_u32(bytes, 20) == Some(0)
        || gpu_u32(bytes, 24) == Some(0)
        || gpu_u32(bytes, 28) == Some(0)
        || gpu_u32(bytes, 36) == Some(0)
        || gpu_u32(bytes, 40) == Some(0)
        || gpu_u32(bytes, 48) != Some(0)
        || gpu_u32(bytes, 52) != Some(0)
    {
        return Err(anyhow!("invalid GPU capabilities record"));
    }
    let scale = f32::from_bits(gpu_u32(bytes, 32).unwrap());
    if !scale.is_finite() || scale <= 0.0 {
        return Err(anyhow!("invalid GPU capabilities scale"));
    }
    let count = gpu_u32(bytes, 44).unwrap() as usize;
    if count > 21 || bytes.len() != HEADER_BYTES + count * ENTRY_BYTES {
        return Err(anyhow!("invalid GPU capabilities limit table"));
    }
    let mut previous = 0;
    for entry in bytes[HEADER_BYTES..].as_chunks::<ENTRY_BYTES>().0 {
        let key = gpu_u16(entry, 0).unwrap();
        if key <= previous
            || key > 21
            || gpu_u16(entry, 2) != Some(0)
            || gpu_u64(entry, 4) == Some(0)
            || gpu_u32(entry, 12) != Some(0)
        {
            return Err(anyhow!("invalid GPU capability limit entry"));
        }
        previous = key;
    }
    Ok(())
}

fn validate_gpu_event(bytes: &[u8]) -> Result<()> {
    if bytes.len() < gpu_wire::GPU_EVENT_HEADER_BYTES
        || bytes.len() > gpu_wire::MAX_GPU_EVENT_BYTES
        || bytes[..4] != gpu_wire::GPU_EVENT_MAGIC
        || gpu_u16(bytes, 4) != Some(gpu_wire::GPU_WIRE_VERSION)
        || !matches!(gpu_u16(bytes, 6), Some(1..=7))
        || gpu_u32(bytes, 8) != Some(bytes.len() as u32)
        || gpu_u32(bytes, 12) != Some(0)
    {
        return Err(anyhow!("invalid GPU event envelope"));
    }
    let event_type = gpu_u16(bytes, 6).unwrap();
    let payload = &bytes[gpu_wire::GPU_EVENT_HEADER_BYTES..];
    match event_type {
        1 | 3 => validate_gpu_text(payload, 8, 12, 16),
        2 => {
            if payload.len() < 32
                || gpu_u16(payload, 6).is_none_or(|flags| flags > 1)
                || gpu_u32(payload, 28) != Some(0)
            {
                return Err(anyhow!("invalid GPU shader diagnostic"));
            }
            validate_gpu_text(payload, 24, 6, 32)
        }
        4 | 7 => validate_gpu_text(payload, 4, 8, 12),
        5 if payload.is_empty() => Ok(()),
        6 => {
            if payload.len() != 28
                || gpu_u32(payload, 0) == Some(0)
                || gpu_u32(payload, 4) == Some(0)
                || gpu_u32(payload, 8) == Some(0)
                || gpu_u32(payload, 12) == Some(0)
                || gpu_u32(payload, 16) == Some(0)
                || !f32::from_bits(gpu_u32(payload, 20).unwrap()).is_finite()
                || f32::from_bits(gpu_u32(payload, 20).unwrap()) <= 0.0
                || !matches!(gpu_u16(payload, 24), Some(1..=4))
                || gpu_u16(payload, 26) != Some(0)
            {
                return Err(anyhow!("invalid GPU surface-change event"));
            }
            Ok(())
        }
        _ => Err(anyhow!("invalid GPU event payload")),
    }
}

fn validate_gpu_text(
    payload: &[u8],
    length_offset: usize,
    flags_offset: usize,
    text_offset: usize,
) -> Result<()> {
    let Some(text_bytes) = gpu_u32(payload, length_offset).map(|value| value as usize) else {
        return Err(anyhow!("truncated GPU text event"));
    };
    let flags = if flags_offset == 6 {
        gpu_u16(payload, flags_offset).map(u32::from)
    } else {
        gpu_u32(payload, flags_offset)
    };
    let padded = text_bytes
        .checked_add(3)
        .map(|value| value & !3)
        .and_then(|value| text_offset.checked_add(value))
        .ok_or_else(|| anyhow!("GPU event text length overflow"))?;
    if text_bytes > gpu_wire::MAX_GPU_DIAGNOSTIC_BYTES
        || flags.is_none_or(|value| value > 1)
        || payload.len() != padded
        || core::str::from_utf8(&payload[text_offset..text_offset + text_bytes]).is_err()
        || payload[text_offset + text_bytes..]
            .iter()
            .any(|byte| *byte != 0)
    {
        return Err(anyhow!("invalid GPU text event"));
    }
    Ok(())
}

fn gpu_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn gpu_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn gpu_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn read_guest_memory(
    instance: &mut polkavm::RawInstance,
    pointer: u32,
    length: usize,
) -> Result<Vec<u8>> {
    if length > MAX_GUEST_READ {
        return Err(anyhow!("guest read exceeds {MAX_GUEST_READ} bytes"));
    }
    let mut bytes = vec![0; length];
    instance
        .read_memory_into(pointer, &mut bytes[..])
        .map_err(|error| anyhow!("read guest memory: {error:?}"))?;
    Ok(bytes)
}

fn map_call_result(result: Result<(), CallError<anyhow::Error>>, phase: &str) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(CallError::Trap) => Err(anyhow!("guest trapped during {phase}")),
        Err(CallError::NotEnoughGas) => Err(anyhow!("guest exhausted gas during {phase}")),
        Err(CallError::Error(error)) => Err(error).context(format!("guest error during {phase}")),
        Err(CallError::User(error)) => Err(error).context(format!("host error during {phase}")),
        Err(error) => Err(anyhow!(
            "unexpected PolkaVM error during {phase}: {error:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use polkavm::Reg;
    use polkavm_common::abi::MemoryMapBuilder;
    use polkavm_common::program::{asm, InstructionSetKind};
    use polkavm_common::writer::ProgramBlobBuilder;

    fn motion_test_program() -> (Vec<u8>, u32) {
        let rw_size = 64 * 1024;
        let stack_size = 4 * 1024;
        let memory = MemoryMapBuilder::new(64 * 1024)
            .rw_data_size(rw_size)
            .stack_size(stack_size)
            .build()
            .unwrap();
        let output = memory.rw_data_address();
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
        builder.set_rw_data_size(rw_size);
        builder.set_stack_size(stack_size);
        builder.add_import(b"host_motion_read");
        builder.add_export_by_basic_block(0, b"init");
        builder.add_export_by_basic_block(0, b"update");
        builder.set_code(
            &[
                asm::load_imm(Reg::A0, output as i32),
                asm::load_imm(Reg::A1, motion_wire::MOTION_SAMPLE_BYTES as i32),
                asm::ecalli(0),
                asm::ret(),
            ],
            &[],
        );
        (builder.into_vec().unwrap(), output)
    }
    fn no_motion_test_program() -> Vec<u8> {
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
        builder.set_stack_size(4 * 1024);
        builder.add_export_by_basic_block(0, b"init");
        builder.add_export_by_basic_block(0, b"update");
        builder.set_code(&[asm::ret()], &[]);
        builder.into_vec().unwrap()
    }

    /// Guest that arms capture during `init` and stores the returned status.
    fn pointer_capture_test_program(request: u32) -> Vec<u8> {
        let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
        builder.set_stack_size(4 * 1024);
        builder.add_import(POINTER_CAPTURE_IMPORT.as_bytes());
        builder.add_export_by_basic_block(0, b"init");
        builder.add_export_by_basic_block(0, b"update");
        builder.set_code(
            &[
                asm::load_imm(Reg::A0, request as i32),
                asm::ecalli(0),
                asm::ret(),
            ],
            &[],
        );
        builder.into_vec().unwrap()
    }

    fn pointer_capture_runtime(request: u32) -> Runtime {
        Runtime::new_with_backend(
            &pointer_capture_test_program(request),
            HashMap::new(),
            PresentationProfile::Framebuffer,
            false,
            1_000_000,
            BackendKind::Interpreter,
        )
        .unwrap()
    }

    #[test]
    fn pointer_capture_import_usage_is_reported() {
        assert!(pointer_capture_runtime(POINTER_CAPTURE_ARM).uses_pointer_capture());
        let plain = Runtime::new_with_backend(
            &no_motion_test_program(),
            HashMap::new(),
            PresentationProfile::Framebuffer,
            false,
            1_000_000,
            BackendKind::Interpreter,
        )
        .unwrap();
        assert!(!plain.uses_pointer_capture());
    }

    #[test]
    fn a_guest_arms_capture_only_where_the_host_supports_it() {
        let mut runtime = pointer_capture_runtime(POINTER_CAPTURE_ARM);
        runtime.init().unwrap();
        assert_eq!(runtime.take_pointer_capture_request(), None);

        runtime.set_pointer_capture_supported(true);
        runtime.update().unwrap();
        assert_eq!(runtime.take_pointer_capture_request(), Some(true));
        assert_eq!(runtime.take_pointer_capture_request(), None);
    }

    #[test]
    fn a_guest_releases_capture_through_the_same_call() {
        let mut runtime = pointer_capture_runtime(POINTER_CAPTURE_RELEASE);
        runtime.set_pointer_capture_supported(true);
        runtime.init().unwrap();
        assert_eq!(runtime.take_pointer_capture_request(), Some(false));
    }

    #[test]
    fn capture_transitions_reach_the_guest_once() {
        let mut runtime = pointer_capture_runtime(POINTER_CAPTURE_ARM);
        runtime.set_pointer_capture_supported(true);
        runtime.set_pointer_capture_active(true).unwrap();
        runtime.set_pointer_capture_active(true).unwrap();
        runtime.set_pointer_capture_active(false).unwrap();
        let records: Vec<_> = runtime.state.input.iter().copied().collect();
        assert_eq!(
            records,
            vec![
                ui::pointer_capture_record(true),
                ui::pointer_capture_record(false)
            ]
        );
    }

    #[test]
    fn an_invalid_capture_request_is_rejected() {
        let mut state = PointerCaptureState {
            supported: true,
            ..PointerCaptureState::default()
        };
        assert_eq!(state.request(7), POINTER_CAPTURE_INVALID_REQUEST);
        assert_eq!(state.request, None);
        assert_eq!(state.request(POINTER_CAPTURE_ARM), POINTER_CAPTURE_ARMED);
        state.set_active(true);
        assert_eq!(state.status(), POINTER_CAPTURE_ACTIVE);
        assert!(!state.armed);
    }

    fn gpu_single_command(opcode: gpu_wire::GpuOpcode, payload: &[u8]) -> Vec<u8> {
        let command_length = gpu_wire::GPU_COMMAND_HEADER_BYTES + payload.len();
        let batch_length = gpu_wire::GPU_BATCH_HEADER_BYTES + command_length;
        let mut bytes = Vec::with_capacity(batch_length);
        bytes.extend_from_slice(&gpu_wire::GPU_WIRE_MAGIC);
        bytes.extend_from_slice(&gpu_wire::GPU_WIRE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(batch_length as u32).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&(opcode as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(command_length as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn runtime_reports_motion_import_usage() {
        let (motion_program, _) = motion_test_program();
        let motion_runtime = Runtime::new_with_backend(
            &motion_program,
            HashMap::new(),
            PresentationProfile::Framebuffer,
            false,
            1_000_000,
            BackendKind::Interpreter,
        )
        .unwrap();
        assert!(motion_runtime.uses_motion());

        let no_motion_runtime = Runtime::new_with_backend(
            &no_motion_test_program(),
            HashMap::new(),
            PresentationProfile::Framebuffer,
            false,
            1_000_000,
            BackendKind::Interpreter,
        )
        .unwrap();
        assert!(!no_motion_runtime.uses_motion());
    }

    #[test]
    fn presentation_profile_uses_tri2d_without_legacy_alias() {
        assert_eq!(
            PresentationProfile::parse("tri2d").expect("tri2d profile should parse"),
            PresentationProfile::Tri2d
        );
        assert!(
            PresentationProfile::parse("ui-mesh").is_err(),
            "the provisional profile name must not remain an alias"
        );
    }

    #[test]
    fn webgpu_raster_rejects_compute_batches() {
        let batch = gpu_single_command(
            gpu_wire::GpuOpcode::DispatchWorkgroups,
            &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0],
        );
        let batch = gpu_wire::decode_gpu_batch(&batch).unwrap();
        assert!(!gpu_batch_supported(
            PresentationProfile::WebGpuRaster,
            &batch
        ));
        assert!(gpu_batch_supported(PresentationProfile::WebGpu, &batch));
    }

    #[test]
    fn motion_state_reports_status_and_consumes_only_successful_reads() {
        let mut motion = MotionState::new();
        assert_eq!(
            motion.read(motion_wire::MOTION_SAMPLE_BYTES),
            Err(motion_wire::MOTION_ERROR_UNAVAILABLE)
        );

        motion.set_availability(motion_wire::MotionAvailability::Available);
        assert_eq!(motion.read(motion_wire::MOTION_SAMPLE_BYTES), Ok(None));
        let sample = motion_wire::MotionSample {
            flags: motion_wire::MOTION_FLAG_ROTATION | motion_wire::MOTION_FLAG_POINTER_EMULATED,
            sequence: 1,
            timestamp_ms: 20.0,
            acceleration_x: 0.0,
            acceleration_y: 0.0,
            acceleration_z: 0.0,
            rotation_alpha: 0.0,
            rotation_beta: -2.0,
            rotation_gamma: 4.0,
        }
        .encode()
        .unwrap();
        motion.set_sample(&sample).unwrap();
        assert_eq!(
            motion.read(motion_wire::MOTION_SAMPLE_BYTES - 1),
            Err(motion_wire::MOTION_ERROR_BUFFER_TOO_SMALL)
        );
        assert_eq!(
            motion.read(motion_wire::MOTION_SAMPLE_BYTES),
            Ok(Some(sample))
        );
        motion.consume();
        assert_eq!(motion.read(motion_wire::MOTION_SAMPLE_BYTES), Ok(None));

        motion.set_availability(motion_wire::MotionAvailability::PermissionDenied);
        assert_eq!(
            motion.read(motion_wire::MOTION_SAMPLE_BYTES),
            Err(motion_wire::MOTION_ERROR_PERMISSION_DENIED)
        );
    }

    #[test]
    fn interpreter_motion_hostcall_reports_status_and_writes_one_sample() {
        let (program, output) = motion_test_program();
        let mut runtime = Runtime::new_with_backend(
            &program,
            HashMap::new(),
            PresentationProfile::Framebuffer,
            false,
            1_000_000,
            BackendKind::Interpreter,
        )
        .unwrap();

        runtime.init().unwrap();
        assert_eq!(
            runtime.instance.reg(Reg::A0) as u32 as i32,
            motion_wire::MOTION_ERROR_UNAVAILABLE
        );
        runtime.set_motion_availability(motion_wire::MotionAvailability::Available);
        runtime.update().unwrap();
        assert_eq!(runtime.instance.reg(Reg::A0), 0);

        let sample = motion_wire::MotionSample {
            flags: motion_wire::MOTION_FLAG_ROTATION | motion_wire::MOTION_FLAG_POINTER_EMULATED,
            sequence: 9,
            timestamp_ms: 30.0,
            acceleration_x: 0.0,
            acceleration_y: 0.0,
            acceleration_z: 0.0,
            rotation_alpha: 0.0,
            rotation_beta: 3.0,
            rotation_gamma: -4.0,
        }
        .encode()
        .unwrap();
        runtime.send_motion_sample(&sample).unwrap();
        runtime.update().unwrap();
        assert_eq!(
            runtime.instance.reg(Reg::A0),
            motion_wire::MOTION_SAMPLE_BYTES as u64
        );
        assert_eq!(
            runtime
                .instance
                .read_memory(output, motion_wire::MOTION_SAMPLE_BYTES as u32)
                .unwrap(),
            sample
        );
        runtime.update().unwrap();
        assert_eq!(runtime.instance.reg(Reg::A0), 0);

        runtime.set_motion_availability(motion_wire::MotionAvailability::PermissionDenied);
        runtime.update().unwrap();
        assert_eq!(
            runtime.instance.reg(Reg::A0) as u32 as i32,
            motion_wire::MOTION_ERROR_PERMISSION_DENIED
        );
    }

    #[test]
    fn input_queue_coalesces_pointer_motion_and_stays_bounded() {
        let mut state = HostState::new(
            HashMap::new(),
            PresentationProfile::Framebuffer,
            false,
            false,
            false,
        );
        for coordinate in 0..10_000u16 {
            state.queue_input(InputEvent {
                event_type: InputEventType::PointerMove,
                code: 0,
                x: coordinate,
                y: coordinate,
            });
        }
        assert_eq!(state.input.len(), 1);
        assert_eq!(
            u16::from_le_bytes(state.input.back().unwrap()[2..4].try_into().unwrap()),
            9_999
        );

        state.queue_input(InputEvent {
            event_type: InputEventType::SurfaceMetrics,
            code: 32,
            x: 1_280,
            y: 800,
        });
        state.queue_input(InputEvent {
            event_type: InputEventType::SurfaceMetrics,
            code: 64,
            x: 2_560,
            y: 1_600,
        });
        assert_eq!(
            state
                .input
                .iter()
                .filter(|event| event[0] == InputEventType::SurfaceMetrics as u8)
                .count(),
            1
        );
        assert_eq!(
            u16::from_le_bytes(state.input.back().unwrap()[2..4].try_into().unwrap()),
            2_560
        );

        for index in 0..(MAX_QUEUED_INPUT_EVENTS + 100) {
            state.queue_input(InputEvent {
                event_type: InputEventType::KeyDown,
                code: index as u8,
                x: 0,
                y: 0,
            });
        }
        assert_eq!(state.input.len(), MAX_QUEUED_INPUT_EVENTS);
        assert_eq!(
            state
                .input
                .iter()
                .filter(|event| event[0] == InputEventType::SurfaceMetrics as u8)
                .count(),
            1
        );
        state.queue_input(InputEvent {
            event_type: InputEventType::SurfaceMetrics,
            code: 64,
            x: 3_000,
            y: 2_000,
        });
        assert_eq!(state.input.len(), MAX_QUEUED_INPUT_EVENTS);
        assert_eq!(
            state.input.back().unwrap()[0],
            InputEventType::SurfaceMetrics as u8
        );
    }

    #[test]
    fn truapi_queues_enforce_frame_count_and_byte_limits() {
        let mut state = HostState::new(
            HashMap::new(),
            PresentationProfile::Framebuffer,
            false,
            false,
            false,
        );
        assert!(state.queue_truapi_response(Vec::new()).is_err());
        assert!(state
            .queue_truapi_response(vec![0; MAX_TRUAPI_FRAME_BYTES + 1])
            .is_err());

        for _ in 0..MAX_QUEUED_TRUAPI_FRAMES {
            state
                .queue_truapi_response(vec![0; 1])
                .expect("bounded response should queue");
        }
        assert!(
            state.queue_truapi_response(vec![0; 1]).is_err(),
            "frame-count overflow must fail closed"
        );

        state.truapi_responses.clear();
        state.truapi_response_bytes = 0;
        for _ in 0..(MAX_QUEUED_TRUAPI_BYTES / MAX_TRUAPI_FRAME_BYTES) {
            state
                .queue_truapi_response(vec![0; MAX_TRUAPI_FRAME_BYTES])
                .expect("response within byte budget should queue");
        }
        assert!(
            state.queue_truapi_response(vec![0; 1]).is_err(),
            "byte-budget overflow must fail closed"
        );

        state.truapi_requests.push_back(vec![1, 2, 3]);
        state.truapi_request_bytes = 3;
        assert_eq!(state.take_truapi_request(), Some(vec![1, 2, 3]));
        assert_eq!(state.truapi_request_bytes, 0);
    }
    #[test]
    fn launch_limits_reject_unbounded_native_inputs() {
        assert!(validate_program_configuration(0, 1).is_err());
        assert!(validate_program_configuration(MAX_PROGRAM_BYTES + 1, 1).is_err());
        assert!(validate_program_configuration(1, 0).is_err());
        assert!(validate_assets(MAX_ASSET_FILES + 1, std::iter::empty()).is_err());
        assert!(validate_assets(1, [("../escape", 1)]).is_err());
        assert!(validate_assets(1, [("asset.bin", MAX_ASSET_FILE_BYTES + 1)]).is_err());
        assert!(validate_assets(2, [("first.bin", MAX_ASSET_BYTES), ("second.bin", 1),],).is_err());
    }

    #[test]
    fn surface_change_rejects_non_positive_and_non_finite_scale() {
        let mut event = vec![0; gpu_wire::GPU_EVENT_HEADER_BYTES + 28];
        event[..4].copy_from_slice(&gpu_wire::GPU_EVENT_MAGIC);
        event[4..6].copy_from_slice(&gpu_wire::GPU_WIRE_VERSION.to_le_bytes());
        event[6..8].copy_from_slice(&(gpu_wire::GpuEventType::SurfaceChanged as u16).to_le_bytes());
        let event_len = event.len() as u32;
        event[8..12].copy_from_slice(&event_len.to_le_bytes());
        let payload = &mut event[gpu_wire::GPU_EVENT_HEADER_BYTES..];
        for offset in [0, 4, 8, 12, 16] {
            payload[offset..offset + 4].copy_from_slice(&1u32.to_le_bytes());
        }
        payload[24..26]
            .copy_from_slice(&(gpu_wire::GpuTextureFormat::Rgba8Unorm as u16).to_le_bytes());

        payload[20..24].copy_from_slice(&1.0f32.to_le_bytes());
        validate_gpu_event(&event).expect("positive finite scale should be valid");
        for scale in [0.0f32, -1.0, f32::NAN, f32::INFINITY] {
            event[gpu_wire::GPU_EVENT_HEADER_BYTES + 20..gpu_wire::GPU_EVENT_HEADER_BYTES + 24]
                .copy_from_slice(&scale.to_le_bytes());
            assert!(
                validate_gpu_event(&event).is_err(),
                "invalid scale {scale:?} must fail closed"
            );
        }
    }
}
