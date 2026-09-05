/* SPDX-License-Identifier: Apache-2.0 OR MIT
 * Vendored from paritytech/polkavm examples/quake/src/vm.rs at
 * 3df1d0309c4c81a1aad0a755d83570d203bba1d9 and adapted for Epoca.
 */

#![allow(non_upper_case_globals)]

use crate::computer::ComputerContext;
use polkavm::{
    Config, Engine, GasMeteringKind, InterruptKind, MemoryAccessError, Module, ModuleConfig,
    ProgramBlob, ProgramCounter, RawInstance, Reg,
};
use std::collections::{BTreeMap, VecDeque};
use std::mem::MaybeUninit;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

struct File {
    blob: Vec<u8>,
}

struct Fd {
    file: Arc<File>,
    position: u64,
}

struct OpenFiles {
    descriptors: BTreeMap<u64, Fd>,
    next: u64,
}

impl OpenFiles {
    fn new() -> Self {
        Self {
            descriptors: BTreeMap::new(),
            next: 3,
        }
    }

    fn open(&mut self, file: Arc<File>) -> Result<u64, u64> {
        if self.descriptors.len() >= MAX_OPEN_FILES {
            return Err(EMFILE);
        }
        let fd = self.next;
        self.next = self.next.checked_add(1).ok_or(EMFILE)?;
        self.descriptors.insert(fd, Fd { file, position: 0 });
        Ok(fd)
    }

    fn get_mut(&mut self, fd: u64) -> Option<&mut Fd> {
        self.descriptors.get_mut(&fd)
    }

    fn remove(&mut self, fd: u64) -> Option<Fd> {
        self.descriptors.remove(&fd)
    }
}

pub struct Vm {
    start: ProgramCounter,
    instance: RawInstance,
    backend: polkavm::BackendKind,
    filesystem: BTreeMap<Vec<u8>, Arc<File>>,
    open_files: OpenFiles,
    input_events: VecDeque<InputEvent>,
    audio_channels: u32,
    epoca_input_events: VecDeque<[u8; crate::INPUT_EVENT_BYTES]>,
    motion: crate::MotionState,
    pointer_capture: crate::PointerCaptureState,
    #[cfg(not(target_arch = "wasm32"))]
    started: Instant,
    #[cfg(target_arch = "wasm32")]
    now_ms: u64,
    host_frame_requests: VecDeque<Vec<u8>>,
    host_frame_request_bytes: usize,
    host_frame_responses: VecDeque<Vec<u8>>,
    host_frame_response_bytes: usize,
    core_arguments: Vec<u8>,
    core_environment: Vec<u8>,
    pub(crate) computer: crate::computer::ComputerDevices,
    computer_calls: BTreeMap<u32, ComputerCall>,

    import_syscall: Option<u32>,
    import_set_palette: Option<u32>,
    import_display: Option<u32>,
    import_fetch_inputs: Option<u32>,
    import_init_audio: Option<u32>,
    import_output_audio: Option<u32>,
    import_epoca_inputs: Option<u32>,
    import_epoca_audio: Option<u32>,
    import_asset_read: Option<u32>,
    import_time_ms: Option<u32>,
    import_log: Option<u32>,
    import_yield: Option<u32>,
    import_host_frame_send: Option<u32>,
    import_motion_read: Option<u32>,
    import_pointer_capture: Option<u32>,
    import_host_frame_poll: Option<u32>,
    import_core_args: Option<u32>,
    import_core_environment: Option<u32>,
    import_core_exit: Option<u32>,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct InputEvent {
    key: u8,
    value: u8,
}

#[derive(Clone, Copy)]
enum ComputerCall {
    Yield,
    ClockMonotonic,
    ClockWall,
    Random,
    TtyCurrent,
    TtyRead,
    TtyWrite,
    TtyGetSize,
    TtySetMode,
    FsOpen,
    FsRead,
    FsWrite,
    FsSeek,
    FsTruncate,
    FsStat,
    FsSync,
    FsClose,
    FsRemove,
    FsList,
    FsMkdir,
    FsRmdir,
    FsRename,
    FsMetadata,
    FsFstat,
    FsListDirectory,
    ProcessRun,
    ProcessSpawn,
    ProcessWait,
    PipeRead,
    PipeWrite,
    PipeClose,
    NetTcpConnect,
    NetRead,
    NetWrite,
    NetClose,
    WorkspaceSpawn,
    WorkspaceSendInput,
    WorkspaceRead,
    WorkspaceResize,
    WorkspaceWait,
    WorkspaceClose,
}

fn computer_call_for(name: &[u8]) -> Option<ComputerCall> {
    Some(match name {
        b"polkadot_host_0_1_core_yield" => ComputerCall::Yield,
        b"polkadot_host_0_1_core_clock_monotonic" => ComputerCall::ClockMonotonic,
        b"polkadot_host_0_1_core_clock_wall" => ComputerCall::ClockWall,
        b"polkadot_host_0_1_core_random" => ComputerCall::Random,
        b"polkadot_host_0_1_tty_current" => ComputerCall::TtyCurrent,
        b"polkadot_host_0_1_tty_read" => ComputerCall::TtyRead,
        b"polkadot_host_0_1_tty_write" => ComputerCall::TtyWrite,
        b"polkadot_host_0_1_tty_get_size" => ComputerCall::TtyGetSize,
        b"polkadot_host_0_1_tty_set_mode" => ComputerCall::TtySetMode,
        b"polkadot_host_0_1_fs_open" => ComputerCall::FsOpen,
        b"polkadot_host_0_1_fs_read" => ComputerCall::FsRead,
        b"polkadot_host_0_1_fs_write" => ComputerCall::FsWrite,
        b"polkadot_host_0_1_fs_seek" => ComputerCall::FsSeek,
        b"polkadot_host_0_1_fs_truncate" => ComputerCall::FsTruncate,
        b"polkadot_host_0_1_fs_stat" => ComputerCall::FsStat,
        b"polkadot_host_0_1_fs_sync" => ComputerCall::FsSync,
        b"polkadot_host_0_1_fs_close" => ComputerCall::FsClose,
        b"polkadot_host_0_1_fs_remove" => ComputerCall::FsRemove,
        b"polkadot_host_0_1_fs_list" => ComputerCall::FsList,
        b"polkadot_host_0_1_fs_mkdir" => ComputerCall::FsMkdir,
        b"polkadot_host_0_1_fs_rmdir" => ComputerCall::FsRmdir,
        b"polkadot_host_0_1_fs_rename" => ComputerCall::FsRename,
        b"polkadot_host_0_1_fs_metadata" => ComputerCall::FsMetadata,
        b"polkadot_host_0_1_fs_fstat" => ComputerCall::FsFstat,
        b"polkadot_host_0_1_fs_list_directory" => ComputerCall::FsListDirectory,
        b"polkadot_host_0_1_process_run" => ComputerCall::ProcessRun,
        b"polkadot_host_0_1_process_spawn" => ComputerCall::ProcessSpawn,
        b"polkadot_host_0_1_process_wait" => ComputerCall::ProcessWait,
        b"polkadot_host_0_1_pipe_read" => ComputerCall::PipeRead,
        b"polkadot_host_0_1_pipe_write" => ComputerCall::PipeWrite,
        b"polkadot_host_0_1_pipe_close" => ComputerCall::PipeClose,
        b"polkadot_host_0_1_net_tcp_connect" => ComputerCall::NetTcpConnect,
        b"polkadot_host_0_1_net_read" => ComputerCall::NetRead,
        b"polkadot_host_0_1_net_write" => ComputerCall::NetWrite,
        b"polkadot_host_0_1_net_close" => ComputerCall::NetClose,
        b"polkadot_host_0_1_workspace_spawn" => ComputerCall::WorkspaceSpawn,
        b"polkadot_host_0_1_workspace_send_input" => ComputerCall::WorkspaceSendInput,
        b"polkadot_host_0_1_workspace_read" => ComputerCall::WorkspaceRead,
        b"polkadot_host_0_1_workspace_resize" => ComputerCall::WorkspaceResize,
        b"polkadot_host_0_1_workspace_wait" => ComputerCall::WorkspaceWait,
        b"polkadot_host_0_1_workspace_close" => ComputerCall::WorkspaceClose,
        _ => return None,
    })
}

fn guest_pointer(value: u64, label: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{label} is out of range"))
}

const SYS_read: u64 = 63;
const SYS_readv: u64 = 65;
const SYS_writev: u64 = 66;
const SYS_exit: u64 = 93;
const SYS_openat: u64 = 56;
const SYS_lseek: u64 = 62;
const SYS_close: u64 = 57;
const SEEK_SET: u64 = 0;
const SEEK_CUR: u64 = 1;
const SEEK_END: u64 = 2;
const FILENO_STDOUT: u64 = 1;
const FILENO_STDERR: u64 = 2;
const ENOSYS: u64 = 38;
const EFAULT: u64 = 14;
const ENOENT: u64 = 2;
const EBADF: u64 = 9;
const EACCES: u64 = 13;
const EINVAL: u64 = 22;
const EMFILE: u64 = 24;
const AT_FDCWD: u64 = (-100_i64) as u64;
const IOV_MAX: u64 = 1024;
const MAX_QUEUED_INPUT_EVENTS: usize = 256;
const MAX_GUEST_WRITE_BYTES: u64 = 4 * 1024;
const MAX_OPEN_FILES: usize = 256;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const AT_PAGESZ: u64 = 6;

fn queue_input_event(events: &mut VecDeque<InputEvent>, key: u8, value: u8) {
    if key == crate::quake_keys::MOUSE_X || key == crate::quake_keys::MOUSE_Y {
        if let Some(event) = events.iter_mut().find(|event| event.key == key) {
            event.value = value;
            return;
        }
    }

    if events.len() == MAX_QUEUED_INPUT_EVENTS {
        events.pop_front();
    }
    events.push_back(InputEvent { key, value });
}

fn queue_epoca_input_event(
    events: &mut VecDeque<[u8; crate::INPUT_EVENT_BYTES]>,
    event: crate::InputEvent,
) {
    if event.event_type == crate::InputEventType::PointerDelta {
        if let Some(queued) = events
            .iter_mut()
            .find(|queued| queued[0] == crate::InputEventType::PointerDelta as u8)
        {
            *queued = event.encode();
            return;
        }
    }

    if events.len() == MAX_QUEUED_INPUT_EVENTS {
        events.pop_front();
    }
    events.push_back(event.encode());
}

fn errno(error: u64) -> u64 {
    (-(error as i64)) as u64
}

fn normalize_path(path: &str) -> String {
    path.trim_start_matches("./")
        .trim_start_matches('/')
        .to_owned()
}

fn seek_position(current: u64, length: u64, offset: i64, whence: u64) -> Result<u64, u64> {
    let base = match whence {
        SEEK_SET => 0,
        SEEK_CUR => current,
        SEEK_END => length,
        _ => return Err(EINVAL),
    };
    u64::try_from(i128::from(base) + i128::from(offset)).map_err(|_| EINVAL)
}

fn queued_input_chunks(
    events: &VecDeque<InputEvent>,
    limit: usize,
) -> (&[InputEvent], &[InputEvent]) {
    let remaining = limit.min(events.len());
    let (first, second) = events.as_slices();
    let first = &first[..first.len().min(remaining)];
    let second = &second[..second.len().min(remaining - first.len())];
    (first, second)
}

fn input_destination(address: u64, event_offset: usize) -> Result<u32, String> {
    let byte_offset = event_offset
        .checked_mul(core::mem::size_of::<InputEvent>())
        .and_then(|offset| u64::try_from(offset).ok())
        .ok_or_else(|| "input address overflow".to_owned())?;
    address
        .checked_add(byte_offset)
        .and_then(|address| u32::try_from(address).ok())
        .ok_or_else(|| "input address is out of range".to_owned())
}

fn write_core_record(
    instance: &mut RawInstance,
    pointer: u64,
    capacity: u64,
    record: &[u8],
) -> Result<u64, String> {
    let required = u32::try_from(record.len())
        .map_err(|_| "computer context record length overflow".to_owned())?;
    if capacity < u64::from(required) {
        let required = i32::try_from(required)
            .map_err(|_| "computer context record length overflow".to_owned())?;
        return Ok(i64::from(-required) as u64);
    }
    let pointer = u32::try_from(pointer)
        .map_err(|_| "computer context destination is out of range".to_owned())?;
    instance
        .write_memory(pointer, record)
        .map_err(|error| error.to_string())?;
    Ok(u64::from(required))
}

pub enum Interruption {
    Exit(i32),
    Yield,
    ProcessRun {
        package: String,
        arguments: Vec<String>,
    },
    ProcessSpawn {
        package: String,
        arguments: Vec<String>,
    },
    ProcessWait {
        pid: u32,
    },
    PipeRead {
        pid: u32,
        destination: u32,
        capacity: usize,
    },
    PipeWrite {
        pid: u32,
        bytes: Vec<u8>,
    },
    PipeClose {
        pid: u32,
    },
    WorkspaceSpawn {
        package: String,
        arguments: Vec<String>,
        columns: u32,
        rows: u32,
    },
    WorkspaceSendInput {
        handle: u32,
        bytes: Vec<u8>,
    },
    WorkspaceRead {
        handle: u32,
        destination: u32,
        capacity: usize,
    },
    WorkspaceResize {
        handle: u32,
        columns: u32,
        rows: u32,
    },
    WorkspaceWait {
        handle: u32,
    },
    WorkspaceClose {
        handle: u32,
    },
    SetPalette {
        palette: Vec<u8>,
    },
    Display {
        width: u64,
        height: u64,
        framebuffer: Vec<u8>,
    },
    AudioInit {
        channels: u32,
        sample_rate: u32,
    },
    AudioFrame {
        buffer: Vec<i16>,
    },
}

impl Vm {
    pub fn from_blob(
        blob: ProgramBlob,
        backend: polkavm::BackendKind,
    ) -> Result<Self, polkavm::Error> {
        let mut config = Config::new();
        config.set_backend(Some(backend));
        config.set_sandboxing_enabled(true);
        #[cfg(target_os = "macos")]
        {
            config.set_sandbox(Some(polkavm::SandboxKind::Generic));
            config.set_allow_experimental(true);
        }
        let engine = Engine::new(&config)?;
        let backend = engine.backend();
        let mut module_config = ModuleConfig::new();
        module_config.set_gas_metering(Some(GasMeteringKind::Sync));
        #[cfg(not(target_arch = "wasm32"))]
        module_config.set_max_heap_size(Some(crate::MAX_GUEST_HEAP_BYTES));
        #[cfg(target_os = "macos")]
        module_config.set_page_size(16_384);
        let module = Module::from_blob(&engine, &module_config, blob)?;

        let start = module
            .exports()
            .find(|export| export.symbol() == "_pvm_start")
            .ok_or_else(|| "'_pvm_start' export not found".to_string())?
            .program_counter();

        let mut import_syscall = None;
        let mut import_set_palette = None;
        let mut import_display = None;
        let mut import_fetch_inputs = None;
        let mut import_init_audio = None;
        let mut import_output_audio = None;
        let mut import_epoca_inputs = None;
        let mut import_epoca_audio = None;
        let mut import_asset_read = None;
        let mut import_time_ms = None;
        let mut import_log = None;
        let mut import_yield = None;
        let mut import_host_frame_send = None;
        let mut import_host_frame_poll = None;
        let mut import_motion_read = None;
        let mut import_pointer_capture = None;
        let mut import_core_args = None;
        let mut import_core_environment = None;
        let mut import_core_exit = None;
        let mut computer_calls = BTreeMap::new();

        for (import_index, import) in module.imports().into_iter().enumerate() {
            let Some(import) = import else {
                continue;
            };

            let import_index = import_index as u32;
            match import.as_bytes() {
                b"pvm_syscall" => import_syscall = Some(import_index),
                b"pvm_set_palette" => import_set_palette = Some(import_index),
                b"pvm_display" => import_display = Some(import_index),
                b"pvm_fetch_inputs" => import_fetch_inputs = Some(import_index),
                b"pvm_init_audio" => import_init_audio = Some(import_index),
                b"pvm_output_audio" => import_output_audio = Some(import_index),
                b"pvm_fetch_epoca_inputs" => import_epoca_inputs = Some(import_index),
                b"host_audio_submit" => import_epoca_audio = Some(import_index),
                b"pvm_asset_read" => import_asset_read = Some(import_index),
                b"pvm_time_ms" => import_time_ms = Some(import_index),
                b"host_log" => import_log = Some(import_index),
                b"pvm_yield" => import_yield = Some(import_index),
                b"host_frame_send" => import_host_frame_send = Some(import_index),
                b"host_frame_poll" => import_host_frame_poll = Some(import_index),
                b"host_motion_read" => import_motion_read = Some(import_index),
                name if name == crate::POINTER_CAPTURE_IMPORT.as_bytes() => {
                    import_pointer_capture = Some(import_index)
                }
                b"polkadot_host_0_1_core_args" => import_core_args = Some(import_index),
                b"polkadot_host_0_1_core_environment" => {
                    import_core_environment = Some(import_index)
                }
                b"polkadot_host_0_1_core_exit" => import_core_exit = Some(import_index),
                name => match computer_call_for(name) {
                    Some(call) => {
                        computer_calls.insert(import_index, call);
                    }
                    None => return Err(format!("unsupported import: {}", import).into()),
                },
            }
        }

        let mut instance = module.instantiate()?;
        instance.set_interpreter_guest_memory_limit(Some(crate::MAX_GUEST_HEAP_BYTES as usize));
        Ok(Self {
            start,
            instance,
            backend,
            filesystem: BTreeMap::new(),
            open_files: OpenFiles::new(),
            input_events: VecDeque::with_capacity(MAX_QUEUED_INPUT_EVENTS),
            audio_channels: 0,
            epoca_input_events: VecDeque::with_capacity(MAX_QUEUED_INPUT_EVENTS),
            motion: crate::MotionState::new(),
            pointer_capture: crate::PointerCaptureState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            started: Instant::now(),
            #[cfg(target_arch = "wasm32")]
            now_ms: 0,
            host_frame_requests: VecDeque::new(),
            host_frame_request_bytes: 0,
            host_frame_responses: VecDeque::new(),
            host_frame_response_bytes: 0,
            core_arguments: Vec::new(),
            core_environment: Vec::new(),
            computer: crate::computer::ComputerDevices::new(),
            computer_calls,
            import_syscall,
            import_set_palette,
            import_display,
            import_fetch_inputs,
            import_init_audio,
            import_output_audio,
            import_epoca_inputs,
            import_epoca_audio,
            import_asset_read,
            import_time_ms,
            import_log,
            import_yield,
            import_host_frame_send,
            import_host_frame_poll,
            import_motion_read,
            import_pointer_capture,
            import_core_args,
            import_core_environment,
            import_core_exit,
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_time_ms(&mut self, time_ms: u64) {
        self.now_ms = self.now_ms.max(time_ms);
    }

    fn time_ms(&self) -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            self.now_ms
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.started.elapsed().as_millis() as u64
        }
    }

    pub fn set_motion_availability(
        &mut self,
        availability: crate::motion_wire::MotionAvailability,
    ) {
        self.motion.set_availability(availability);
    }

    pub fn send_motion_sample(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.motion
            .set_sample(bytes)
            .map_err(|error| error.to_string())
    }

    pub fn backend(&self) -> polkavm::BackendKind {
        self.backend
    }

    pub fn uses_motion(&self) -> bool {
        self.import_motion_read.is_some()
    }

    pub fn uses_pointer_capture(&self) -> bool {
        self.import_pointer_capture.is_some()
    }

    pub fn set_pointer_capture_supported(&mut self, supported: bool) {
        self.pointer_capture.supported = supported;
        if !supported {
            self.pointer_capture.armed = false;
            self.pointer_capture.request = None;
        }
    }

    pub fn set_pointer_capture_active(&mut self, active: bool) -> Result<(), String> {
        if self.pointer_capture.active == active {
            return Ok(());
        }
        if self.epoca_input_events.len() == MAX_QUEUED_INPUT_EVENTS {
            return Err("pointer capture input queue is full".into());
        }
        self.epoca_input_events
            .push_back(crate::ui::pointer_capture_record(active));
        self.pointer_capture.set_active(active);
        Ok(())
    }

    pub fn take_pointer_capture_request(&mut self) -> Option<bool> {
        self.pointer_capture.request.take()
    }

    pub fn take_host_frame_request(&mut self) -> Option<Vec<u8>> {
        let frame = self.host_frame_requests.pop_front()?;
        self.host_frame_request_bytes -= frame.len();
        Some(frame)
    }

    pub fn send_host_frame_response(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > crate::MAX_HOST_FRAME_BYTES {
            return Err("invalid host-frame response frame".into());
        }
        if self.host_frame_responses.len() == crate::MAX_QUEUED_HOST_FRAMES
            || self.host_frame_response_bytes.saturating_add(bytes.len())
                > crate::MAX_QUEUED_HOST_FRAME_BYTES
        {
            return Err("host-frame response queue overflow".into());
        }
        self.host_frame_response_bytes += bytes.len();
        self.host_frame_responses.push_back(bytes);
        Ok(())
    }

    pub fn set_gas(&mut self, gas: u64) {
        self.instance.set_gas(gas.min(i64::MAX as u64) as i64);
    }
    pub fn gas_remaining(&self) -> u64 {
        self.instance.gas().max(0) as u64
    }

    fn send_input_event(&mut self, key: u8, value: u8) {
        queue_input_event(&mut self.input_events, key, value);
    }

    pub fn send_key(&mut self, key: u8, is_pressed: bool) {
        self.send_input_event(key, if is_pressed { 1 } else { 0 });
    }

    pub fn send_mouse_move(&mut self, delta_x: i8, delta_y: i8) {
        if delta_x != 0 {
            self.send_input_event(crate::quake_keys::MOUSE_X, delta_x as u8);
        }

        if delta_y != 0 {
            self.send_input_event(crate::quake_keys::MOUSE_Y, delta_y as u8);
        }
    }
    pub fn uses_epoca_inputs(&self) -> bool {
        self.import_epoca_inputs.is_some()
    }

    pub fn send_epoca_input(&mut self, event: crate::InputEvent) {
        queue_epoca_input_event(&mut self.epoca_input_events, event);
    }

    fn read_cstr(&mut self, address: u64) -> Result<Option<Vec<u8>>, String> {
        // FIXME: This is slow.
        let mut buffer = Vec::new();
        for offset in 0..255 {
            let Some(address) = address
                .checked_add(offset)
                .and_then(|address| u32::try_from(address).ok())
            else {
                return Ok(None);
            };
            match self.instance.read_u8(address) {
                Ok(byte) => {
                    if byte == 0 {
                        return Ok(Some(buffer));
                    }

                    buffer.push(byte)
                }
                Err(MemoryAccessError::Error(error)) => return Err(error.into()),
                Err(MemoryAccessError::OutOfRangeAccess { .. }) => return Ok(None),
                Err(MemoryAccessError::MemoryLimitReached) => return Ok(None),
            }
        }

        Ok(None)
    }

    pub fn register_file(&mut self, path: &str, blob: Vec<u8>) {
        let normalized = normalize_path(path);
        self.filesystem
            .insert(normalized.into_bytes(), Arc::new(File { blob }));
    }

    fn handle_open(&mut self, path: &[u8], flags: u64) -> u64 {
        let path = normalize_path(&String::from_utf8_lossy(path));
        log::debug!("Open: path={path:?}, flags=0x{flags:x}");

        if let Some(file) = self.filesystem.get(path.as_bytes()) {
            if (flags & (O_WRONLY | O_RDWR)) != 0 {
                return errno(EACCES);
            }

            return match self.open_files.open(Arc::clone(file)) {
                Ok(fd) => fd,
                Err(error) => errno(error),
            };
        }

        errno(ENOENT)
    }

    fn handle_lseek(&mut self, fd: u64, offset: i64, whence: u64) -> u64 {
        log::trace!("Seek: fd={fd}, offset={offset}, whence={whence}");

        let Some(fd) = self.open_files.get_mut(fd) else {
            log::trace!("  -> BADF");
            return errno(EBADF);
        };

        let Ok(position) = seek_position(fd.position, fd.file.blob.len() as u64, offset, whence)
        else {
            log::trace!("  -> EINVAL");
            return errno(EINVAL);
        };
        fd.position = position;

        log::trace!("  -> offset={}", fd.position);
        fd.position
    }

    fn handle_read(&mut self, fd: u64, address: u64, length: u64) -> Result<u64, String> {
        log::trace!("Read: fd={fd}, address=0x{address:x}, length={length}");

        let Some(fd) = self.open_files.get_mut(fd) else {
            log::trace!("  -> EBADF");
            return Ok(errno(EBADF));
        };

        if address.checked_add(length).is_none() || u32::try_from(address + length).is_err() {
            log::trace!("  -> EFAULT");
            return Ok(errno(EFAULT));
        }

        let Ok(address) = u32::try_from(address) else {
            log::trace!("  -> EFAULT");
            return Ok(errno(EFAULT));
        };

        let end = core::cmp::min(fd.position.wrapping_add(length), fd.file.blob.len() as u64);
        if fd.position >= end || fd.position >= fd.file.blob.len() as u64 {
            log::trace!("  -> offset={}, length=0", fd.position);
            return Ok(0);
        }

        let blob = &fd.file.blob[fd.position as usize..end as usize];
        match self.instance.write_memory(address, blob) {
            Ok(()) => {}
            Err(MemoryAccessError::Error(error)) => return Err(error.into()),
            Err(MemoryAccessError::OutOfRangeAccess { .. }) => {
                log::trace!("  -> EFAULT");
                return Ok(errno(EFAULT));
            }
            Err(MemoryAccessError::MemoryLimitReached) => return Ok(errno(EFAULT)),
        }

        let length_out = blob.len() as u64;
        log::trace!(
            "  -> offset={}, length={}, new offset={}",
            fd.position,
            length_out,
            fd.position + length_out
        );

        fd.position += length_out;
        Ok(length_out)
    }

    fn handle_write(&mut self, fd: u64, address: u64, length: u64) -> Result<u64, String> {
        if fd != FILENO_STDOUT && fd != FILENO_STDERR {
            return Ok(errno(EBADF));
        }

        let length = length.min(MAX_GUEST_WRITE_BYTES);
        if address.checked_add(length).is_none() || u32::try_from(address + length).is_err() {
            return Ok(errno(EFAULT));
        }

        let Ok(address) = u32::try_from(address) else {
            return Ok(errno(EFAULT));
        };

        let data = match self.instance.read_memory(address, length as u32) {
            Ok(data) => data,
            Err(MemoryAccessError::Error(error)) => return Err(error.into()),
            Err(MemoryAccessError::OutOfRangeAccess { .. })
            | Err(MemoryAccessError::MemoryLimitReached) => return Ok(errno(EFAULT)),
        };
        eprint!("{}", String::from_utf8_lossy(&data));
        Ok(length)
    }

    fn handle_close(&mut self, fd: u64) -> u64 {
        log::debug!("Close: fd = {fd}");
        let Some(_fd) = self.open_files.remove(fd) else {
            log::trace!("  -> EBADF");
            return errno(EBADF);
        };

        0
    }

    #[allow(non_upper_case_globals)]
    pub fn setup(&mut self, context: ComputerContext) -> Result<(), String> {
        let ComputerContext {
            arguments,
            environment,
            encoded_arguments,
            encoded_environment,
        } = context;
        let argc = arguments.len() as u64;
        let envp_len = environment.len() as u64;
        let auxv: &[(u64, u64)] = &[(AT_PAGESZ, 4096)];
        let auxv_len = auxv.len() as u64;

        let mut sp = self.instance.module().default_sp();

        sp -= (1 + argc + 1 + envp_len + 1 + (auxv_len + 1) * 2) * 8;
        let address_init = sp;

        let mut p = sp;
        self.instance.write_u64(p as u32, argc)?;
        p += 8;

        for argument in &arguments {
            sp -= argument.len() as u64 + 1;
            self.instance.write_memory(sp as u32, argument.as_bytes())?;
            self.instance.write_u64(p as u32, sp)?;
            p += 8;
        }
        p += 8; // Null pointer.

        for (key, value) in &environment {
            sp -= key.len() as u64 + value.len() as u64 + 2;
            self.instance.write_memory(sp as u32, key.as_bytes())?;
            self.instance
                .write_memory((sp + key.len() as u64) as u32, b"=")?;
            self.instance
                .write_memory((sp + key.len() as u64 + 1) as u32, value.as_bytes())?;
            self.instance.write_u64(p as u32, sp)?;
            p += 8;
        }
        p += 8; // Null pointer.

        for &(key, value) in auxv {
            self.instance.write_u64(p as u32, key)?;
            p += 8;
            self.instance.write_u64(p as u32, value)?;
            p += 8;
        }

        self.core_arguments = encoded_arguments;
        self.core_environment = encoded_environment;
        self.instance.set_reg(Reg::SP, sp);
        self.instance.set_reg(Reg::A0, address_init);
        self.instance.set_reg(Reg::RA, polkavm::RETURN_TO_HOST);
        self.instance.set_next_program_counter(self.start);
        Ok(())
    }

    pub fn run(&mut self) -> Result<Interruption, String> {
        'outer_loop: loop {
            #[allow(clippy::redundant_guards)] // Disable buggy lint.
            match self.instance.run()? {
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_core_args => {
                    let pointer = self.instance.reg(Reg::A0);
                    let capacity = self.instance.reg(Reg::A1);
                    let result = write_core_record(
                        &mut self.instance,
                        pointer,
                        capacity,
                        &self.core_arguments,
                    )?;
                    self.instance.set_reg(Reg::A0, result);
                    continue;
                }
                InterruptKind::Ecalli(hostcall)
                    if Some(hostcall) == self.import_core_environment =>
                {
                    let pointer = self.instance.reg(Reg::A0);
                    let capacity = self.instance.reg(Reg::A1);
                    let result = write_core_record(
                        &mut self.instance,
                        pointer,
                        capacity,
                        &self.core_environment,
                    )?;
                    self.instance.set_reg(Reg::A0, result);
                    continue;
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_core_exit => {
                    let status = self.instance.reg(Reg::A0) as u32 as i32;
                    return Ok(Interruption::Exit(status));
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_set_palette => {
                    let address = self.instance.reg(Reg::A0);
                    log::debug!("Set palette called: 0x{:x}", address);
                    let palette = self.instance.read_memory(address as u32, 256 * 3)?;
                    return Ok(Interruption::SetPalette { palette });
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_display => {
                    let width = self.instance.reg(Reg::A0);
                    let height = self.instance.reg(Reg::A1);
                    let address = self.instance.reg(Reg::A2);
                    log::trace!("Display called: {}x{}, 0x{:x}", width, height, address);
                    let pixels = width
                        .checked_mul(height)
                        .ok_or_else(|| "frame dimensions overflow".to_owned())?;
                    if pixels == 0 || pixels > (crate::MAX_FRAME_BYTES / 4) as u64 {
                        return Err("frame dimensions exceed the host limit".into());
                    }
                    let address = u32::try_from(address)
                        .map_err(|_| "frame address is out of range".to_owned())?;
                    let framebuffer = self.instance.read_memory(address, pixels as u32)?;
                    return Ok(Interruption::Display {
                        width,
                        height,
                        framebuffer,
                    });
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_epoca_inputs => {
                    let address = u32::try_from(self.instance.reg(Reg::A0))
                        .map_err(|_| "input address is out of range".to_owned())?;
                    let capacity =
                        usize::try_from(self.instance.reg(Reg::A1)).unwrap_or(usize::MAX);
                    let event_count =
                        (capacity / crate::INPUT_EVENT_BYTES).min(self.epoca_input_events.len());
                    let mut written = 0usize;
                    for _ in 0..event_count {
                        let Some(event) = self.epoca_input_events.pop_front() else {
                            break;
                        };
                        let destination = address
                            .checked_add(written as u32)
                            .ok_or_else(|| "input destination overflow".to_owned())?;
                        self.instance.write_memory(destination, &event)?;
                        written += event.len();
                    }
                    self.instance.set_reg(Reg::A0, written as u64);
                    continue;
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_motion_read => {
                    let address = u32::try_from(self.instance.reg(Reg::A0))
                        .map_err(|_| "motion output address is out of range".to_owned())?;
                    let capacity =
                        usize::try_from(self.instance.reg(Reg::A1)).unwrap_or(usize::MAX);
                    let sample = match self.motion.read(capacity) {
                        Ok(Some(sample)) => sample,
                        Ok(None) => {
                            self.instance
                                .set_reg(Reg::A0, crate::motion_wire::MOTION_READ_NO_SAMPLE as u64);
                            continue;
                        }
                        Err(status) => {
                            self.instance.set_reg(Reg::A0, status as i64 as u64);
                            continue;
                        }
                    };
                    if self.instance.write_memory(address, &sample).is_err() {
                        self.instance.set_reg(
                            Reg::A0,
                            crate::motion_wire::MOTION_ERROR_INVALID_GUEST_RANGE as i64 as u64,
                        );
                        continue;
                    }
                    self.motion.consume();
                    self.instance
                        .set_reg(Reg::A0, crate::motion_wire::MOTION_SAMPLE_BYTES as u64);
                    continue;
                }
                InterruptKind::Ecalli(hostcall)
                    if Some(hostcall) == self.import_pointer_capture =>
                {
                    let request = self.instance.reg(Reg::A0) as u32;
                    let status = self.pointer_capture.request(request);
                    self.instance.set_reg(Reg::A0, status as i64 as u64);
                    continue;
                }
                InterruptKind::Ecalli(hostcall)
                    if Some(hostcall) == self.import_host_frame_send =>
                {
                    let address = u32::try_from(self.instance.reg(Reg::A0))
                        .map_err(|_| "host-frame request address is out of range".to_owned())?;
                    let length = usize::try_from(self.instance.reg(Reg::A1)).unwrap_or(usize::MAX);
                    if length == 0 || length > crate::MAX_HOST_FRAME_BYTES {
                        self.instance.set_reg(Reg::A0, 1);
                        continue;
                    }
                    if self.host_frame_requests.len() == crate::MAX_QUEUED_HOST_FRAMES
                        || self.host_frame_request_bytes.saturating_add(length)
                            > crate::MAX_QUEUED_HOST_FRAME_BYTES
                    {
                        self.instance.set_reg(Reg::A0, 2);
                        continue;
                    }
                    let bytes = self.instance.read_memory(address, length as u32)?;
                    self.host_frame_request_bytes += bytes.len();
                    self.host_frame_requests.push_back(bytes);
                    self.instance.set_reg(Reg::A0, 0);
                    continue;
                }
                InterruptKind::Ecalli(hostcall)
                    if Some(hostcall) == self.import_host_frame_poll =>
                {
                    let address = u32::try_from(self.instance.reg(Reg::A0))
                        .map_err(|_| "host-frame response address is out of range".to_owned())?;
                    let capacity =
                        usize::try_from(self.instance.reg(Reg::A1)).unwrap_or(usize::MAX);
                    let Some(required) = self.host_frame_responses.front().map(Vec::len) else {
                        self.instance.set_reg(Reg::A0, 0);
                        continue;
                    };
                    if capacity < required {
                        let required = i32::try_from(required)
                            .map_err(|_| "host-frame response length overflow".to_owned())?;
                        self.instance.set_reg(Reg::A0, i64::from(-required) as u64);
                        continue;
                    }
                    let response = self.host_frame_responses.front().unwrap();
                    self.instance.write_memory(address, response)?;
                    self.host_frame_responses.pop_front();
                    self.host_frame_response_bytes -= required;
                    self.instance.set_reg(Reg::A0, required as u64);
                    continue;
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_asset_read => {
                    let name_address = u32::try_from(self.instance.reg(Reg::A0))
                        .map_err(|_| "asset name address is out of range".to_owned())?;
                    let name_length = usize::try_from(self.instance.reg(Reg::A1))
                        .unwrap_or(usize::MAX)
                        .min(crate::MAX_ASSET_NAME_BYTES);
                    let offset = usize::try_from(self.instance.reg(Reg::A2)).unwrap_or(usize::MAX);
                    let destination = u32::try_from(self.instance.reg(Reg::A3))
                        .map_err(|_| "asset destination is out of range".to_owned())?;
                    let capacity = usize::try_from(self.instance.reg(Reg::A4))
                        .unwrap_or(usize::MAX)
                        .min(crate::MAX_ASSET_READ_BYTES);
                    let name = self
                        .instance
                        .read_memory(name_address, name_length as u32)?;
                    let Some(file) = self.filesystem.get(&name) else {
                        self.instance.set_reg(Reg::A0, 0);
                        continue;
                    };
                    let length = capacity.min(file.blob.len().saturating_sub(offset));
                    if length == 0 {
                        self.instance.set_reg(Reg::A0, 0);
                        continue;
                    }
                    self.instance
                        .write_memory(destination, &file.blob[offset..offset + length])?;
                    self.instance.set_reg(Reg::A0, length as u64);
                    continue;
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_time_ms => {
                    self.instance.set_reg(Reg::A0, self.time_ms());
                    continue;
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_log => {
                    let address = u32::try_from(self.instance.reg(Reg::A0))
                        .map_err(|_| "log address is out of range".to_owned())?;
                    let length =
                        u32::try_from(self.instance.reg(Reg::A1).min(crate::MAX_LOG_BYTES as u64))
                            .map_err(|_| "log length is out of range".to_owned())?;
                    let message = self.instance.read_memory(address, length)?;
                    log::info!(target: "epoca_pvm_guest", "{}", String::from_utf8_lossy(&message));
                    continue;
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_yield => {
                    return Ok(Interruption::Yield);
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_epoca_audio => {
                    let address = u32::try_from(self.instance.reg(Reg::A0))
                        .map_err(|_| "audio address is out of range".to_owned())?;
                    let sample_count =
                        usize::try_from(self.instance.reg(Reg::A1)).unwrap_or(usize::MAX);
                    if sample_count == 0
                        || sample_count % crate::AUDIO_CHANNELS as usize != 0
                        || sample_count > crate::MAX_AUDIO_SAMPLES_PER_CALL
                    {
                        self.instance.set_reg(Reg::A0, 1);
                        continue;
                    }
                    let mut buffer = vec![0i16; sample_count];
                    self.instance.read_memory_into(address, unsafe {
                        core::slice::from_raw_parts_mut(
                            buffer.as_mut_ptr().cast::<u8>(),
                            sample_count * core::mem::size_of::<i16>(),
                        )
                    })?;
                    self.instance.set_reg(Reg::A0, 0);
                    return Ok(Interruption::AudioFrame { buffer });
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_fetch_inputs => {
                    let address = self.instance.reg(Reg::A0);
                    let requested =
                        usize::try_from(self.instance.reg(Reg::A1)).unwrap_or(usize::MAX);
                    let (first, second) = queued_input_chunks(&self.input_events, requested);
                    let mut written = 0usize;

                    for events in [first, second] {
                        if events.is_empty() {
                            continue;
                        }
                        let address = input_destination(address, written)?;
                        self.instance.write_memory(address, unsafe {
                            core::slice::from_raw_parts(
                                events.as_ptr().cast::<u8>(),
                                core::mem::size_of_val(events),
                            )
                        })?;
                        written += events.len();
                    }

                    for _ in 0..written {
                        self.input_events.pop_front();
                    }
                    self.instance.set_reg(Reg::A0, written as u64);
                    continue;
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_init_audio => {
                    let channels = self.instance.reg(Reg::A0) as u32;
                    let bits_per_sample = self.instance.reg(Reg::A1);
                    let sample_rate = self.instance.reg(Reg::A2) as u32;
                    if bits_per_sample != 16 {
                        self.instance.set_reg(Reg::A0, 0);
                        continue;
                    }

                    self.audio_channels = channels;
                    self.instance.set_reg(Reg::A0, 1);
                    return Ok(Interruption::AudioInit {
                        channels,
                        sample_rate,
                    });
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_output_audio => {
                    let address = self.instance.reg(Reg::A0);
                    let samples = self.instance.reg(Reg::A1) as usize;
                    let channels = self.audio_channels as usize;
                    let length = samples.saturating_mul(channels).min(1024 * 64);
                    let address = u32::try_from(address)
                        .map_err(|_| "audio address is out of range".to_owned())?;
                    let mut buffer: Vec<i16> = Vec::with_capacity(length);
                    unsafe {
                        self.instance.read_memory_into(
                            address,
                            core::slice::from_raw_parts_mut(
                                buffer
                                    .spare_capacity_mut()
                                    .as_mut_ptr()
                                    .cast::<MaybeUninit<u8>>(),
                                length * core::mem::size_of::<i16>(),
                            ),
                        )?;
                        buffer.set_len(length);
                    }

                    return Ok(Interruption::AudioFrame { buffer });
                }
                InterruptKind::Ecalli(hostcall) if Some(hostcall) == self.import_syscall => {
                    let syscall = self.instance.reg(Reg::A0);
                    let a1 = self.instance.reg(Reg::A1);
                    let a2 = self.instance.reg(Reg::A2);
                    let a3 = self.instance.reg(Reg::A3);
                    let a4 = self.instance.reg(Reg::A4);
                    let a5 = self.instance.reg(Reg::A5);
                    let pc = self.instance.program_counter();
                    log::trace!(
                        "Syscall at pc={pc:?}: {syscall:>3}, args = [0x{a1:>016x}, 0x{a2:>016x}, 0x{a3:>016x}, 0x{a4:>016x}, 0x{a5:>016x}]"
                    );

                    match syscall {
                        SYS_read => {
                            let result = self.handle_read(a1, a2, a3)?;
                            self.instance.set_reg(Reg::A0, result);
                            continue;
                        }
                        SYS_readv => {
                            if a3 == 0 || a3 > IOV_MAX {
                                self.instance.set_reg(Reg::A0, errno(EINVAL));
                                continue;
                            }

                            let mut total_length = 0u64;
                            for n in 0..a3 {
                                let address =
                                    self.instance.read_u64(a2.wrapping_add(n * 16) as u32)?;
                                let length = self
                                    .instance
                                    .read_u64(a2.wrapping_add(n * 16).wrapping_add(8) as u32)?;
                                let bytes_read = self.handle_read(a1, address, length)?;
                                if (bytes_read as i64) < 0 {
                                    self.instance.set_reg(Reg::A0, bytes_read);
                                    continue 'outer_loop;
                                }

                                total_length = total_length
                                    .checked_add(bytes_read)
                                    .ok_or_else(|| "readv byte count overflow".to_owned())?;
                                if bytes_read < length {
                                    break;
                                }
                            }

                            self.instance.set_reg(Reg::A0, total_length);
                            continue;
                        }
                        SYS_writev => {
                            if a3 == 0 || a3 > IOV_MAX {
                                self.instance.set_reg(Reg::A0, errno(EINVAL));
                                continue;
                            }

                            let mut total_length = 0u64;
                            for n in 0..a3 {
                                let address =
                                    self.instance.read_u64(a2.wrapping_add(n * 16) as u32)?;
                                let length = self
                                    .instance
                                    .read_u64(a2.wrapping_add(n * 16).wrapping_add(8) as u32)?;
                                let bytes_written = self.handle_write(a1, address, length)?;
                                if (bytes_written as i64) < 0 {
                                    self.instance.set_reg(Reg::A0, bytes_written);
                                    continue 'outer_loop;
                                }

                                total_length = total_length
                                    .checked_add(bytes_written)
                                    .ok_or_else(|| "writev byte count overflow".to_owned())?;
                                if bytes_written < length {
                                    break;
                                }
                            }

                            self.instance.set_reg(Reg::A0, total_length);
                            continue;
                        }
                        SYS_exit => {
                            let status = i32::try_from(a1)
                                .map_err(|_| format!("exit status is out of range: {a1}"))?;
                            log::info!("Exit called: status={status}");
                            return Ok(Interruption::Exit(status));
                        }
                        SYS_openat => {
                            if a1 == AT_FDCWD {
                                let Some(path) = self.read_cstr(a2)? else {
                                    self.instance.set_reg(Reg::A0, errno(EFAULT));
                                    continue;
                                };

                                let result = self.handle_open(&path, a3);
                                self.instance.set_reg(Reg::A0, result);
                                continue;
                            }
                        }
                        SYS_lseek => {
                            let result = self.handle_lseek(a1, a2 as i64, a3);
                            self.instance.set_reg(Reg::A0, result);
                            continue;
                        }
                        SYS_close => {
                            let result = self.handle_close(a1);
                            self.instance.set_reg(Reg::A0, result);
                            continue;
                        }
                        _ => {
                            log::debug!("Unimplemented syscall at pc={pc:?}: {syscall:>3}, args = [0x{a1:>016x}, 0x{a2:>016x}, 0x{a3:>016x}, 0x{a4:>016x}, 0x{a5:>016x}]");
                        }
                    }

                    self.instance.set_reg(Reg::A0, errno(ENOSYS));
                }
                InterruptKind::Finished => {
                    return Ok(Interruption::Exit(0));
                }
                InterruptKind::Ecalli(hostcall) => {
                    let Some(call) = self.computer_calls.get(&hostcall).copied() else {
                        return Err(format!("unsupported host call: {hostcall}"));
                    };
                    if let Some(interruption) = self.handle_computer_call(call)? {
                        return Ok(interruption);
                    }
                    continue;
                }
                InterruptKind::Trap => {
                    return Err(format!(
                        "execution trapped at {:?}",
                        self.instance.program_counter()
                    ));
                }
                InterruptKind::NotEnoughGas => {
                    return Err("ran out of gas".into());
                }
                InterruptKind::Segfault(address) => {
                    return Err(format!("guest segfault at {address:?}"));
                }
                InterruptKind::Step => return Err("unexpected guest step".into()),
            }
        }
    }

    fn handle_computer_call(&mut self, call: ComputerCall) -> Result<Option<Interruption>, String> {
        const MAX_TTY_TRANSFER: usize = 64 * 1024;
        const MAX_FS_TRANSFER: usize = 1024 * 1024;

        let a0 = self.instance.reg(Reg::A0);
        let a1 = self.instance.reg(Reg::A1);
        let a2 = self.instance.reg(Reg::A2);
        let status = |value: i32| i64::from(value) as u64;

        match call {
            ComputerCall::Yield => return Ok(Some(Interruption::Yield)),
            ComputerCall::ClockMonotonic => {
                let destination = guest_pointer(a0, "monotonic clock destination")?;
                self.instance.write_memory(
                    destination,
                    &self.computer.core_clock_monotonic().to_le_bytes(),
                )?;
                self.instance.set_reg(Reg::A0, 0);
            }
            ComputerCall::ClockWall => {
                let destination = guest_pointer(a0, "wall clock destination")?;
                self.instance
                    .write_memory(destination, &self.computer.core_clock_wall().to_le_bytes())?;
                self.instance.set_reg(Reg::A0, 0);
            }
            ComputerCall::Random => {
                let destination = guest_pointer(a0, "random destination")?;
                let length = usize::try_from(a1).unwrap_or(usize::MAX);
                let result = if length == 0 {
                    crate::computer::STATUS_INVALID
                } else if length > crate::computer::MAX_RANDOM_BYTES {
                    crate::computer::STATUS_LIMIT
                } else {
                    let mut bytes = vec![0u8; length];
                    let result = self.computer.core_random(&mut bytes);
                    if result == 0 {
                        self.instance.write_memory(destination, &bytes)?;
                    }
                    result
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::TtyCurrent => {
                self.instance
                    .set_reg(Reg::A0, u64::from(crate::computer::COMPUTER_TTY_HANDLE));
            }
            ComputerCall::TtyRead => {
                let handle = a0 as u32;
                let destination = guest_pointer(a1, "tty read destination")?;
                let capacity = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_TTY_TRANSFER);
                let result = if capacity == 0 {
                    crate::computer::STATUS_INVALID
                } else {
                    let mut buffer = vec![0u8; capacity];
                    let result = self.computer.tty_read_into(handle, &mut buffer);
                    if result > 0 {
                        self.instance
                            .write_memory(destination, &buffer[..result as usize])?;
                    }
                    result
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::TtyWrite => {
                let handle = a0 as u32;
                let source = guest_pointer(a1, "tty write source")?;
                let length = usize::try_from(a2).unwrap_or(usize::MAX);
                if length > MAX_TTY_TRANSFER {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_LIMIT));
                } else {
                    let bytes = self.instance.read_memory(source, length as u32)?;
                    let result = self.computer.tty_write(handle, &bytes);
                    self.instance.set_reg(Reg::A0, status(result));
                }
            }
            ComputerCall::TtyGetSize => {
                let handle = a0 as u32;
                let record = guest_pointer(a1, "tty size record")?;
                if handle != crate::computer::COMPUTER_TTY_HANDLE {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_BAD_HANDLE));
                } else {
                    let (columns, rows) = self.computer.terminal_size();
                    let mut bytes = [0u8; 8];
                    bytes[..4].copy_from_slice(&columns.to_le_bytes());
                    bytes[4..].copy_from_slice(&rows.to_le_bytes());
                    self.instance.write_memory(record, &bytes)?;
                    self.instance.set_reg(Reg::A0, 0);
                }
            }
            ComputerCall::TtySetMode => {
                let result = self.computer.tty_set_mode(a0 as u32, a1 as u32);
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsOpen => {
                let path = self.read_computer_path(a0, a1)?;
                let result = match path {
                    Some(path) => self.computer.fs_open(&path, a2 as u32),
                    None => crate::computer::STATUS_INVALID,
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsRead => {
                let handle = a0 as u32;
                let destination = guest_pointer(a1, "file read destination")?;
                let capacity = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_FS_TRANSFER);
                let mut buffer = vec![0u8; capacity];
                let result = self.computer.fs_read(handle, &mut buffer);
                if result > 0 {
                    self.instance
                        .write_memory(destination, &buffer[..result as usize])?;
                }
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsWrite => {
                let handle = a0 as u32;
                let source = guest_pointer(a1, "file write source")?;
                let length = usize::try_from(a2).unwrap_or(usize::MAX);
                if length > MAX_FS_TRANSFER {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_LIMIT));
                } else {
                    let bytes = self.instance.read_memory(source, length as u32)?;
                    let result = self.computer.fs_write(handle, &bytes);
                    self.instance.set_reg(Reg::A0, status(result));
                }
            }
            ComputerCall::FsSeek => {
                let result = self
                    .computer
                    .fs_seek(a0 as u32, a1 as u32 as i32, a2 as u32);
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsTruncate => {
                let result = self.computer.fs_truncate(a0 as u32, a1 as u32);
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsStat => {
                let path = self.read_computer_path(a0, a1)?;
                let record = guest_pointer(a2, "stat record")?;
                let result = match path.as_deref().and_then(|path| self.computer.fs_stat(path)) {
                    Some(size) => {
                        self.instance.write_memory(record, &size.to_le_bytes())?;
                        0
                    }
                    None => crate::computer::STATUS_NOT_FOUND,
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsSync => {
                let result = self.computer.fs_sync(a0 as u32);
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsClose => {
                let result = self.computer.fs_close(a0 as u32);
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsRemove => {
                let result = self
                    .read_computer_path(a0, a1)?
                    .as_deref()
                    .map_or(crate::computer::STATUS_INVALID, |path| {
                        self.computer.fs_remove(path)
                    });
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsList => {
                let destination = guest_pointer(a0, "list destination")?;
                let capacity = usize::try_from(a1)
                    .unwrap_or(usize::MAX)
                    .min(MAX_FS_TRANSFER);
                let record = self.computer.fs_list_record();
                if record.len() > capacity {
                    let required = i32::try_from(record.len())
                        .map_err(|_| "listing record length overflow".to_owned())?;
                    self.instance.set_reg(Reg::A0, status(-required));
                } else {
                    self.instance.write_memory(destination, &record)?;
                    self.instance.set_reg(Reg::A0, record.len() as u64);
                }
            }
            ComputerCall::FsMkdir | ComputerCall::FsRmdir => {
                let path = self.read_computer_path(a0, a1)?;
                let result = match path {
                    Some(path) => match call {
                        ComputerCall::FsMkdir => self.computer.filesystem.mkdir(&path),
                        _ => self.computer.filesystem.rmdir(&path),
                    },
                    None => crate::computer::STATUS_INVALID,
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsRename => {
                let a3 = self.instance.reg(Reg::A3);
                let old = self.read_computer_path(a0, a1)?;
                let new = self.read_computer_path(a2, a3)?;
                let result = match (old, new) {
                    (Some(old), Some(new)) => self.computer.filesystem.rename(&old, &new),
                    _ => crate::computer::STATUS_INVALID,
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsMetadata | ComputerCall::FsFstat => {
                let (record, result) = match call {
                    ComputerCall::FsMetadata => {
                        let path = self.read_computer_path(a0, a1)?;
                        let result = match path {
                            Some(path) => self.computer.filesystem.metadata(&path),
                            None => Err(crate::computer::STATUS_INVALID),
                        };
                        (guest_pointer(a2, "metadata record")?, result)
                    }
                    _ => (
                        guest_pointer(a1, "fstat record")?,
                        self.computer.filesystem.fstat(a0 as u32),
                    ),
                };
                let result = match result {
                    Ok(bytes) => {
                        self.instance.write_memory(record, &bytes)?;
                        0
                    }
                    Err(status) => status,
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::FsListDirectory => {
                let a3 = self.instance.reg(Reg::A3);
                let path = self.read_computer_path(a0, a1)?;
                let destination = guest_pointer(a2, "directory listing destination")?;
                let capacity = usize::try_from(a3)
                    .unwrap_or(usize::MAX)
                    .min(MAX_FS_TRANSFER);
                let result = match path {
                    Some(path) => self.computer.filesystem.list_directory(&path),
                    None => Err(crate::computer::STATUS_INVALID),
                };
                let result = match result {
                    Ok(record) if record.len() > capacity => -(record.len() as i32),
                    Ok(record) => {
                        self.instance.write_memory(destination, &record)?;
                        record.len() as i32
                    }
                    Err(status) => status,
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::ProcessRun => {
                let a3 = self.instance.reg(Reg::A3);
                let Some(package) = self.read_computer_string(a0, a1, 64)? else {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                };
                let Some(arguments) = self.read_string_record(a2, a3)? else {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                };
                return Ok(Some(Interruption::ProcessRun { package, arguments }));
            }
            ComputerCall::ProcessSpawn => {
                let a3 = self.instance.reg(Reg::A3);
                let Some(package) = self.read_computer_string(a0, a1, 64)? else {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                };
                let Some(arguments) = self.read_string_record(a2, a3)? else {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                };
                return Ok(Some(Interruption::ProcessSpawn { package, arguments }));
            }
            ComputerCall::ProcessWait => {
                return Ok(Some(Interruption::ProcessWait { pid: a0 as u32 }));
            }
            ComputerCall::PipeRead => {
                let destination = guest_pointer(a1, "pipe read destination")?;
                let capacity = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_TTY_TRANSFER);
                if capacity == 0 {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                }
                return Ok(Some(Interruption::PipeRead {
                    pid: a0 as u32,
                    destination,
                    capacity,
                }));
            }
            ComputerCall::PipeWrite => {
                let source = guest_pointer(a1, "pipe write source")?;
                let length = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_TTY_TRANSFER);
                let bytes = self.instance.read_memory(source, length as u32)?;
                return Ok(Some(Interruption::PipeWrite {
                    pid: a0 as u32,
                    bytes,
                }));
            }
            ComputerCall::PipeClose => {
                return Ok(Some(Interruption::PipeClose { pid: a0 as u32 }));
            }
            ComputerCall::WorkspaceSpawn => {
                let a3 = self.instance.reg(Reg::A3);
                let a4 = self.instance.reg(Reg::A4);
                let a5 = self.instance.reg(Reg::A5);
                let Some(package) = self.read_computer_string(a0, a1, 64)? else {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                };
                let Some(arguments) = self.read_string_record(a2, a3)? else {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                };
                let (Ok(columns), Ok(rows)) = (u32::try_from(a4), u32::try_from(a5)) else {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                };
                return Ok(Some(Interruption::WorkspaceSpawn {
                    package,
                    arguments,
                    columns,
                    rows,
                }));
            }
            ComputerCall::WorkspaceSendInput => {
                let source = guest_pointer(a1, "workspace input source")?;
                let length = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_TTY_TRANSFER);
                let bytes = self.instance.read_memory(source, length as u32)?;
                return Ok(Some(Interruption::WorkspaceSendInput {
                    handle: a0 as u32,
                    bytes,
                }));
            }
            ComputerCall::WorkspaceRead => {
                let destination = guest_pointer(a1, "workspace read destination")?;
                let capacity = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_TTY_TRANSFER);
                if capacity == 0 {
                    self.instance
                        .set_reg(Reg::A0, status(crate::computer::STATUS_INVALID));
                    return Ok(None);
                }
                return Ok(Some(Interruption::WorkspaceRead {
                    handle: a0 as u32,
                    destination,
                    capacity,
                }));
            }
            ComputerCall::WorkspaceResize => {
                return Ok(Some(Interruption::WorkspaceResize {
                    handle: a0 as u32,
                    columns: a1 as u32,
                    rows: a2 as u32,
                }));
            }
            ComputerCall::WorkspaceWait => {
                return Ok(Some(Interruption::WorkspaceWait { handle: a0 as u32 }));
            }
            ComputerCall::WorkspaceClose => {
                return Ok(Some(Interruption::WorkspaceClose { handle: a0 as u32 }));
            }
            ComputerCall::NetTcpConnect => {
                let address =
                    self.read_computer_string(a0, a1, crate::computer::MAX_NET_ADDRESS_BYTES)?;
                let result = match address {
                    Some(address) => self.computer.net_tcp_connect(&address),
                    None => crate::computer::STATUS_INVALID,
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::NetRead => {
                let handle = a0 as u32;
                let destination = guest_pointer(a1, "net read destination")?;
                let capacity = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_TTY_TRANSFER);
                let result = if capacity == 0 {
                    crate::computer::STATUS_INVALID
                } else {
                    let mut buffer = vec![0u8; capacity];
                    let received = self.computer.net_read(handle, &mut buffer);
                    if received > 0 {
                        self.instance
                            .write_memory(destination, &buffer[..received as usize])?;
                    }
                    received
                };
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::NetWrite => {
                let handle = a0 as u32;
                let source = guest_pointer(a1, "net write source")?;
                let length = usize::try_from(a2)
                    .unwrap_or(usize::MAX)
                    .min(MAX_TTY_TRANSFER);
                let bytes = self.instance.read_memory(source, length as u32)?;
                let result = self.computer.net_write(handle, &bytes);
                self.instance.set_reg(Reg::A0, status(result));
            }
            ComputerCall::NetClose => {
                let result = self.computer.net_close(a0 as u32);
                self.instance.set_reg(Reg::A0, status(result));
            }
        }
        Ok(None)
    }

    fn read_computer_string(
        &mut self,
        pointer: u64,
        length: u64,
        maximum: usize,
    ) -> Result<Option<String>, String> {
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        if length == 0 || length > maximum {
            return Ok(None);
        }
        let pointer = guest_pointer(pointer, "string pointer")?;
        let bytes = self.instance.read_memory(pointer, length as u32)?;
        Ok(String::from_utf8(bytes).ok())
    }

    fn read_string_record(
        &mut self,
        pointer: u64,
        length: u64,
    ) -> Result<Option<Vec<String>>, String> {
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        if length > 4 * 1024 {
            return Ok(None);
        }
        if length == 0 {
            return Ok(Some(Vec::new()));
        }
        let pointer = guest_pointer(pointer, "record pointer")?;
        let bytes = self.instance.read_memory(pointer, length as u32)?;
        if bytes.len() < 4 {
            return Ok(None);
        }
        let count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        if count > 16 {
            return Ok(None);
        }
        let mut entries = Vec::with_capacity(count);
        let mut cursor = 4usize;
        for _ in 0..count {
            let Some(end) = cursor.checked_add(4) else {
                return Ok(None);
            };
            let Some(header) = bytes.get(cursor..end) else {
                return Ok(None);
            };
            let entry_length =
                u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
            cursor = end;
            let Some(entry_end) = cursor.checked_add(entry_length) else {
                return Ok(None);
            };
            let Some(entry) = bytes.get(cursor..entry_end) else {
                return Ok(None);
            };
            let Ok(entry) = String::from_utf8(entry.to_vec()) else {
                return Ok(None);
            };
            entries.push(entry);
            cursor = entry_end;
        }
        Ok(Some(entries))
    }

    /// Completes a pending `process_run`-family hostcall with a status value.
    pub(crate) fn resolve_process_run(&mut self, result: i32) {
        self.instance.set_reg(Reg::A0, i64::from(result) as u64);
    }

    /// Completes a pending `pipe_read` hostcall by delivering `bytes`.
    pub(crate) fn resolve_pipe_read(
        &mut self,
        destination: u32,
        bytes: &[u8],
    ) -> Result<(), String> {
        self.instance
            .write_memory(destination, bytes)
            .map_err(|error| error.to_string())?;
        self.instance.set_reg(Reg::A0, bytes.len() as u64);
        Ok(())
    }
    fn read_computer_path(&mut self, pointer: u64, length: u64) -> Result<Option<String>, String> {
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        if length == 0 || length > crate::computer::MAX_COMPUTER_PATH_BYTES {
            return Ok(None);
        }
        let pointer = guest_pointer(pointer, "path pointer")?;
        let bytes = self.instance.read_memory(pointer, length as u32)?;
        Ok(String::from_utf8(bytes).ok())
    }
}

#[cfg(test)]
mod tests;
