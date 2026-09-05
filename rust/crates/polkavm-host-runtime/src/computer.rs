/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::corevm::{Interruption, Vm};
use crate::filesystem::{lock, FileSession, FilesystemMetadata};
use anyhow::{anyhow, bail, Context, Result};
use polkavm::ProgramBlob;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

/// Version of the experimental Polkadot Host application-computer contract.
pub const COMPUTER_ABI_VERSION: (u16, u16) = (0, 1);

/// Maximum encoded argument or environment record accepted at launch.
pub const MAX_COMPUTER_CONTEXT_BYTES: usize = 64 * 1024;

/// Maximum number of arguments or environment entries accepted at launch.
pub const MAX_COMPUTER_CONTEXT_ENTRIES: usize = 1_024;

/// Terminal handle granted to every computer guest.
pub const COMPUTER_TTY_HANDLE: u32 = 1;
/// Raw (non-canonical) terminal input mode flag.
pub const TTY_MODE_RAW: u32 = 1;
/// Terminal echo mode flag.
pub const TTY_MODE_ECHO: u32 = 2;
/// Open flag granting read access.
pub const FS_OPEN_READ: u32 = 1;
/// Open flag granting write access.
pub const FS_OPEN_WRITE: u32 = 2;
/// Open flag creating a missing file when writable.
pub const FS_OPEN_CREATE: u32 = 4;
/// Open flag truncating an existing writable file.
pub const FS_OPEN_TRUNCATE: u32 = 8;
/// Open flag requiring atomic creation of a new writable file.
pub const FS_OPEN_EXCLUSIVE: u32 = 16;
/// Open flag positioning every write at the shared file's current end.
pub const FS_OPEN_APPEND: u32 = 32;

/// Maximum bytes queued toward the guest terminal.
pub const MAX_TTY_INPUT_BYTES: usize = 64 * 1024;
/// Maximum guest terminal output retained per run.
pub const MAX_TTY_OUTPUT_BYTES: usize = 1024 * 1024;
/// Maximum files in the mounted computer filesystem.
pub const MAX_COMPUTER_FILES: usize = 64;
/// Maximum directories, excluding the implicit `/home` root.
pub const MAX_COMPUTER_DIRECTORIES: usize = 256;
/// Maximum size of one mounted file.
pub const MAX_COMPUTER_FILE_BYTES: usize = 1024 * 1024;
/// Maximum simultaneously open computer file handles.
pub const MAX_OPEN_COMPUTER_FILES: usize = 16;
/// Maximum accepted file path length in bytes.
pub const MAX_COMPUTER_PATH_BYTES: usize = 200;
/// Maximum simultaneously open outbound TCP sockets.
pub const MAX_OPEN_SOCKETS: usize = 4;
/// First handle value assigned to network sockets.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const FIRST_SOCKET_HANDLE: u32 = 0x1000;
/// First handle value assigned to open files.
pub(crate) const FIRST_FILE_HANDLE: u32 = 16;
/// Maximum accepted network address length in bytes.
pub const MAX_NET_ADDRESS_BYTES: usize = 256;
/// Maximum random bytes filled by one hostcall.
pub const MAX_RANDOM_BYTES: usize = 4 * 1024;

pub(crate) const STATUS_WOULD_BLOCK: i32 = -1;
pub(crate) const STATUS_BAD_HANDLE: i32 = -2;
pub(crate) const STATUS_INVALID: i32 = -3;
pub(crate) const STATUS_NOT_FOUND: i32 = -4;
pub(crate) const STATUS_DENIED: i32 = -5;
pub(crate) const STATUS_LIMIT: i32 = -6;
pub(crate) const STATUS_EXISTS: i32 = -7;
pub(crate) const STATUS_NOT_DIRECTORY: i32 = -8;
pub(crate) const STATUS_IS_DIRECTORY: i32 = -9;
pub(crate) const STATUS_NOT_EMPTY: i32 = -10;

/// Launch context exposed through `polkadot-host-computer/0.1/core`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerContext {
    pub(crate) arguments: Vec<String>,
    pub(crate) environment: Vec<(String, String)>,
    pub(crate) encoded_arguments: Vec<u8>,
    pub(crate) encoded_environment: Vec<u8>,
}

impl ComputerContext {
    /// Validates and encodes an application-computer launch context.
    pub fn new(arguments: Vec<String>, environment: Vec<(String, String)>) -> Result<Self> {
        if arguments.len() > MAX_COMPUTER_CONTEXT_ENTRIES {
            bail!("computer argument count exceeds the host limit");
        }
        if environment.len() > MAX_COMPUTER_CONTEXT_ENTRIES {
            bail!("computer environment count exceeds the host limit");
        }

        for argument in &arguments {
            if argument.as_bytes().contains(&0) {
                bail!("computer arguments must not contain NUL bytes");
            }
        }

        let mut keys = BTreeSet::new();
        for (key, value) in &environment {
            if key.is_empty() || key.contains('=') || key.as_bytes().contains(&0) {
                bail!(
                    "computer environment keys must be non-empty and contain neither '=' nor NUL"
                );
            }
            if value.as_bytes().contains(&0) {
                bail!("computer environment values must not contain NUL bytes");
            }
            if !keys.insert(key.as_str()) {
                bail!("computer environment contains duplicate key {key:?}");
            }
        }

        let encoded_arguments = encode_arguments(&arguments)?;
        let encoded_environment = encode_environment(&environment)?;
        Ok(Self {
            arguments,
            environment,
            encoded_arguments,
            encoded_environment,
        })
    }

    /// Returns launch arguments in guest-visible order.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Returns launch environment entries in guest-visible order.
    pub fn environment(&self) -> &[(String, String)] {
        &self.environment
    }
}

impl Default for ComputerContext {
    fn default() -> Self {
        Self::new(Vec::new(), Vec::new()).expect("an empty computer context is valid")
    }
}

/// Observable state after running an application-computer guest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputerStatus {
    /// The guest yielded control and may be resumed.
    Yielded,
    /// The guest exited with the supplied application status.
    Exited(i32),
    /// The guest requested a child process; the supervisor must resolve it.
    SpawnRequested,
    /// The guest issued a spawn/wait/pipe call; the supervisor must resolve it.
    ChildRequest,
    /// A spawn named an unregistered package and open resolution is
    /// enabled; the embedder must `provide_package` or `reject_package`.
    PackageRequested,
}

/// Terminal, filesystem, and network devices granted to one computer guest.
pub(crate) struct ComputerDevices {
    tty_input: VecDeque<u8>,
    tty_input_closed: bool,
    tty_output: Vec<u8>,
    tty_columns: u32,
    tty_rows: u32,
    tty_mode: u32,
    pub(crate) filesystem: FileSession,
    network_enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    monotonic_epoch: std::time::Instant,
    #[cfg(not(target_arch = "wasm32"))]
    sockets: BTreeMap<u32, std::net::TcpStream>,
    #[cfg(not(target_arch = "wasm32"))]
    next_socket: u32,
}

impl ComputerDevices {
    pub(crate) fn new() -> Self {
        Self {
            tty_input: VecDeque::new(),
            tty_input_closed: false,
            tty_output: Vec::new(),
            tty_columns: 80,
            tty_rows: 24,
            tty_mode: TTY_MODE_ECHO,
            filesystem: FileSession::new(),
            network_enabled: false,
            #[cfg(not(target_arch = "wasm32"))]
            monotonic_epoch: std::time::Instant::now(),
            #[cfg(not(target_arch = "wasm32"))]
            sockets: BTreeMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            next_socket: FIRST_SOCKET_HANDLE,
        }
    }

    /* ── Core clocks and entropy ──────────────────────────────────── */

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn core_clock_monotonic(&self) -> u64 {
        self.monotonic_epoch
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn core_clock_monotonic(&self) -> u64 {
        0
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn core_clock_wall(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(u64::MAX as u128) as u64
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn core_clock_wall(&self) -> u64 {
        0
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn core_random(&self, bytes: &mut [u8]) -> i32 {
        if bytes.is_empty() {
            return STATUS_INVALID;
        }
        match getrandom::fill(bytes) {
            Ok(()) => 0,
            Err(_) => STATUS_INVALID,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn core_random(&self, _bytes: &mut [u8]) -> i32 {
        STATUS_DENIED
    }

    /* ── Network boundary (host.net v0: outbound TCP only) ─────────── */

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn net_tcp_connect(&mut self, address: &str) -> i32 {
        use std::net::ToSocketAddrs;
        if !self.network_enabled {
            return STATUS_DENIED;
        }
        if self.sockets.len() >= MAX_OPEN_SOCKETS {
            return STATUS_LIMIT;
        }
        let Ok(mut resolved) = address.to_socket_addrs() else {
            return STATUS_NOT_FOUND;
        };
        let Some(target) = resolved.next() else {
            return STATUS_NOT_FOUND;
        };
        let Ok(stream) =
            std::net::TcpStream::connect_timeout(&target, std::time::Duration::from_secs(5))
        else {
            return STATUS_INVALID;
        };
        if stream.set_nonblocking(true).is_err() {
            return STATUS_INVALID;
        }
        let handle = self.next_socket;
        self.next_socket += 1;
        self.sockets.insert(handle, stream);
        handle as i32
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn net_read(&mut self, handle: u32, buffer: &mut [u8]) -> i32 {
        use std::io::Read;
        let Some(stream) = self.sockets.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        match stream.read(buffer) {
            Ok(count) => count as i32,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => STATUS_WOULD_BLOCK,
            Err(_) => STATUS_INVALID,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn net_write(&mut self, handle: u32, bytes: &[u8]) -> i32 {
        use std::io::Write;
        let Some(stream) = self.sockets.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        match stream.write(bytes) {
            Ok(count) => count as i32,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => STATUS_WOULD_BLOCK,
            Err(_) => STATUS_INVALID,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn net_close(&mut self, handle: u32) -> i32 {
        if self.sockets.remove(&handle).is_none() {
            return STATUS_BAD_HANDLE;
        }
        0
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn net_tcp_connect(&mut self, _address: &str) -> i32 {
        STATUS_DENIED
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn net_read(&mut self, _handle: u32, _buffer: &mut [u8]) -> i32 {
        STATUS_DENIED
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn net_write(&mut self, _handle: u32, _bytes: &[u8]) -> i32 {
        STATUS_DENIED
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn net_close(&mut self, _handle: u32) -> i32 {
        STATUS_DENIED
    }

    fn push_terminal_input(&mut self, bytes: &[u8]) -> Result<()> {
        if self.tty_input_closed {
            bail!("terminal input is closed");
        }
        if self.tty_input.len().saturating_add(bytes.len()) > MAX_TTY_INPUT_BYTES {
            bail!("terminal input queue limit exceeded");
        }
        self.tty_input.extend(bytes.iter().copied());
        Ok(())
    }

    /// Marks the input stream as ended; reads on an empty queue return 0.
    fn close_input(&mut self) {
        self.tty_input_closed = true;
    }

    fn input_space(&self) -> usize {
        if self.tty_input_closed {
            return 0;
        }
        MAX_TTY_INPUT_BYTES.saturating_sub(self.tty_input.len())
    }

    fn take_terminal_output(&mut self) -> Option<Vec<u8>> {
        if self.tty_output.is_empty() {
            return None;
        }
        Some(core::mem::take(&mut self.tty_output))
    }

    fn mount_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        lock(&self.filesystem.shared).mount_file(path, bytes)
    }

    fn take_modified_files(&mut self) -> Vec<(String, Vec<u8>)> {
        lock(&self.filesystem.shared).take_modified_files()
    }

    fn take_removed_files(&mut self) -> Vec<String> {
        lock(&self.filesystem.shared).take_removed_files()
    }

    pub(crate) fn has_terminal_input(&self) -> bool {
        !self.tty_input.is_empty()
    }

    pub(crate) fn terminal_mode(&self) -> u32 {
        self.tty_mode
    }

    pub(crate) fn terminal_size(&self) -> (u32, u32) {
        (self.tty_columns, self.tty_rows)
    }

    pub(crate) fn tty_read_into(&mut self, handle: u32, buffer: &mut [u8]) -> i32 {
        if handle != COMPUTER_TTY_HANDLE {
            return STATUS_BAD_HANDLE;
        }
        if buffer.is_empty() {
            return STATUS_INVALID;
        }
        if self.tty_input.is_empty() {
            if self.tty_input_closed {
                return 0;
            }
            return STATUS_WOULD_BLOCK;
        }
        let mut written = 0usize;
        while written < buffer.len() {
            let Some(byte) = self.tty_input.pop_front() else {
                break;
            };
            buffer[written] = byte;
            written += 1;
        }
        written as i32
    }

    pub(crate) fn tty_write(&mut self, handle: u32, bytes: &[u8]) -> i32 {
        if handle != COMPUTER_TTY_HANDLE {
            return STATUS_BAD_HANDLE;
        }
        let available = MAX_TTY_OUTPUT_BYTES.saturating_sub(self.tty_output.len());
        let written = bytes.len().min(available);
        self.tty_output.extend_from_slice(&bytes[..written]);
        written as i32
    }

    pub(crate) fn tty_set_mode(&mut self, handle: u32, flags: u32) -> i32 {
        if handle != COMPUTER_TTY_HANDLE {
            return STATUS_BAD_HANDLE;
        }
        if flags & !(TTY_MODE_RAW | TTY_MODE_ECHO) != 0 {
            return STATUS_INVALID;
        }
        self.tty_mode = flags;
        0
    }

    pub(crate) fn fs_open(&mut self, path: &str, flags: u32) -> i32 {
        self.filesystem.open(path, flags)
    }

    pub(crate) fn fs_read(&mut self, handle: u32, buffer: &mut [u8]) -> i32 {
        self.filesystem.read(handle, buffer)
    }

    pub(crate) fn fs_write(&mut self, handle: u32, bytes: &[u8]) -> i32 {
        self.filesystem.write(handle, bytes)
    }

    pub(crate) fn fs_seek(&mut self, handle: u32, offset: i32, whence: u32) -> i32 {
        self.filesystem.seek(handle, offset, whence)
    }

    pub(crate) fn fs_truncate(&mut self, handle: u32, length: u32) -> i32 {
        self.filesystem.truncate(handle, length)
    }

    pub(crate) fn fs_stat(&self, path: &str) -> Option<u32> {
        self.filesystem.stat(path)
    }

    /// Makes shared writes visible; does not guarantee durable persistence.
    pub(crate) fn fs_sync(&mut self, handle: u32) -> i32 {
        self.filesystem.sync(handle)
    }

    pub(crate) fn fs_close(&mut self, handle: u32) -> i32 {
        self.filesystem.close(handle)
    }

    pub(crate) fn fs_remove(&mut self, path: &str) -> i32 {
        self.filesystem.remove(path)
    }

    /// Encodes all file paths (not directories), preserving the original ABI.
    pub(crate) fn fs_list_record(&self) -> Vec<u8> {
        self.filesystem.list()
    }
}

/// A spawn/wait/pipe operation awaiting supervisor resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChildProcessRequest {
    Spawn {
        package: String,
        arguments: Vec<String>,
    },
    Wait {
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
}

/// Experimental host-neutral runtime for `polkadot-host-computer/0.1` guests.
pub struct ComputerRuntime {
    vm: Vm,
    max_gas_per_run: u64,
    exit_status: Option<i32>,
    pending_spawn: Option<(String, Vec<String>)>,
    pending_child_request: Option<ChildProcessRequest>,
}

impl ComputerRuntime {
    /// Creates a runtime using the preferred backend for this platform.
    pub fn new(program: &[u8], context: ComputerContext, max_gas_per_run: u64) -> Result<Self> {
        Self::new_with_backend(
            program,
            context,
            max_gas_per_run,
            crate::preferred_backend(),
        )
    }

    /// Creates a runtime using an explicitly selected PolkaVM backend.
    pub fn new_with_backend(
        program: &[u8],
        context: ComputerContext,
        max_gas_per_run: u64,
        backend: polkavm::BackendKind,
    ) -> Result<Self> {
        crate::validate_launch_inputs(program, &HashMap::new(), max_gas_per_run)?;
        let blob = ProgramBlob::parse(program.into()).context("parse PolkaVM computer program")?;
        crate::validate_blob(&blob)?;
        if !blob.exports().any(|export| export.symbol() == "_pvm_start") {
            bail!("computer guests must export '_pvm_start'");
        }

        let mut vm = Vm::from_blob(blob, backend).context("create PolkaVM computer guest")?;
        vm.setup(context).map_err(|error| anyhow!(error))?;
        Ok(Self {
            vm,
            max_gas_per_run,
            exit_status: None,
            pending_spawn: None,
            pending_child_request: None,
        })
    }

    /// Runs until the guest yields, exits, or fails.
    pub fn run(&mut self) -> Result<ComputerStatus> {
        if let Some(status) = self.exit_status {
            return Ok(ComputerStatus::Exited(status));
        }

        self.vm.set_gas(self.max_gas_per_run);
        let interruption = match self.vm.run() {
            Ok(interruption) => interruption,
            Err(error) => {
                self.dispose();
                return Err(anyhow!(error));
            }
        };
        match interruption {
            Interruption::Exit(status) => {
                self.exit_status = Some(status);
                self.dispose();
                Ok(ComputerStatus::Exited(status))
            }
            Interruption::Yield => Ok(ComputerStatus::Yielded),
            Interruption::ProcessRun { package, arguments } => {
                self.pending_spawn = Some((package, arguments));
                Ok(ComputerStatus::SpawnRequested)
            }
            Interruption::ProcessSpawn { package, arguments } => {
                self.pending_child_request =
                    Some(ChildProcessRequest::Spawn { package, arguments });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::ProcessWait { pid } => {
                self.pending_child_request = Some(ChildProcessRequest::Wait { pid });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::PipeRead {
                pid,
                destination,
                capacity,
            } => {
                self.pending_child_request = Some(ChildProcessRequest::PipeRead {
                    pid,
                    destination,
                    capacity,
                });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::PipeWrite { pid, bytes } => {
                self.pending_child_request = Some(ChildProcessRequest::PipeWrite { pid, bytes });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::PipeClose { pid } => {
                self.pending_child_request = Some(ChildProcessRequest::PipeClose { pid });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::WorkspaceSpawn {
                package,
                arguments,
                columns,
                rows,
            } => {
                self.pending_child_request = Some(ChildProcessRequest::WorkspaceSpawn {
                    package,
                    arguments,
                    columns,
                    rows,
                });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::WorkspaceSendInput { handle, bytes } => {
                self.pending_child_request =
                    Some(ChildProcessRequest::WorkspaceSendInput { handle, bytes });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::WorkspaceRead {
                handle,
                destination,
                capacity,
            } => {
                self.pending_child_request = Some(ChildProcessRequest::WorkspaceRead {
                    handle,
                    destination,
                    capacity,
                });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::WorkspaceResize {
                handle,
                columns,
                rows,
            } => {
                self.pending_child_request = Some(ChildProcessRequest::WorkspaceResize {
                    handle,
                    columns,
                    rows,
                });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::WorkspaceWait { handle } => {
                self.pending_child_request = Some(ChildProcessRequest::WorkspaceWait { handle });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::WorkspaceClose { handle } => {
                self.pending_child_request = Some(ChildProcessRequest::WorkspaceClose { handle });
                Ok(ComputerStatus::ChildRequest)
            }
            Interruption::SetPalette { .. }
            | Interruption::Display { .. }
            | Interruption::AudioInit { .. }
            | Interruption::AudioFrame { .. } => {
                self.dispose();
                bail!("computer guest requested an application-presentation operation")
            }
        }
    }

    fn dispose(&mut self) {
        self.vm.computer.filesystem.close_all();
        self.set_network_enabled(false);
        self.exit_status.get_or_insert(130);
    }

    /// Returns the selected execution backend.
    pub fn backend(&self) -> polkavm::BackendKind {
        self.vm.backend()
    }

    /// Takes the pending spawn request after `SpawnRequested`.
    pub fn take_spawn_request(&mut self) -> Option<(String, Vec<String>)> {
        self.pending_spawn.take()
    }

    /// Takes the pending spawn/wait/pipe request after `ChildRequest`.
    pub fn take_child_request(&mut self) -> Option<ChildProcessRequest> {
        self.pending_child_request.take()
    }

    /// Completes a pending `pipe_read` by copying `bytes` into guest memory.
    pub fn resolve_read(&mut self, destination: u32, bytes: &[u8]) -> Result<()> {
        self.vm
            .resolve_pipe_read(destination, bytes)
            .map_err(|error| anyhow!(error))
    }

    /// Marks the guest input stream as ended; reads then report EOF.
    pub fn close_terminal_input(&mut self) {
        self.vm.computer.close_input();
    }

    /// Returns remaining space in the guest input queue.
    pub fn terminal_input_space(&self) -> usize {
        self.vm.computer.input_space()
    }

    /// Completes a pending spawn with the child's exit status or an error.
    pub fn resolve_spawn(&mut self, result: i32) {
        self.vm.resolve_process_run(result);
    }

    /// Queues keyboard bytes toward the guest terminal.
    pub fn send_terminal_input(&mut self, bytes: &[u8]) -> Result<()> {
        self.vm.computer.push_terminal_input(bytes)
    }

    /// Drains ANSI output produced by the guest terminal.
    pub fn take_terminal_output(&mut self) -> Option<Vec<u8>> {
        self.vm.computer.take_terminal_output()
    }

    /// Returns whether undelivered terminal input remains queued.
    pub fn has_terminal_input(&self) -> bool {
        self.vm.computer.has_terminal_input()
    }

    /// Sets the terminal dimensions observed by the guest.
    pub fn set_terminal_size(&mut self, columns: u32, rows: u32) -> Result<()> {
        if columns == 0 || rows == 0 || columns > 1_000 || rows > 1_000 {
            bail!("invalid terminal size {columns}x{rows}");
        }
        self.vm.computer.tty_columns = columns;
        self.vm.computer.tty_rows = rows;
        Ok(())
    }

    /// Grants or revokes the outbound TCP capability. Revocation also
    /// closes every socket opened while the capability was held.
    pub fn set_network_enabled(&mut self, enabled: bool) {
        self.vm.computer.network_enabled = enabled;
        #[cfg(not(target_arch = "wasm32"))]
        if !enabled {
            self.vm.computer.sockets.clear();
        }
    }

    /// Returns whether the guest's terminal input stream has been closed.
    pub fn terminal_input_closed(&self) -> bool {
        self.vm.computer.tty_input_closed
    }

    /// Returns the current guest terminal mode flags.
    pub fn terminal_mode(&self) -> u32 {
        self.vm.computer.terminal_mode()
    }

    /// Mounts one file into the guest `/home` filesystem.
    pub fn mount_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.vm.computer.mount_file(path, bytes)
    }

    /// Drains files modified by the guest since the previous call.
    pub fn take_modified_files(&mut self) -> Vec<(String, Vec<u8>)> {
        self.vm.computer.take_modified_files()
    }

    /// Drains paths removed by the guest since the previous call.
    pub fn take_removed_files(&mut self) -> Vec<String> {
        self.vm.computer.take_removed_files()
    }

    /// Exports stable identities and times; persist with both byte delta drains.
    pub fn export_filesystem_metadata(&self) -> FilesystemMetadata {
        lock(&self.vm.computer.filesystem.shared).export_metadata()
    }

    /// Drains metadata only after a guest namespace or content mutation.
    pub fn take_filesystem_metadata(&mut self) -> Option<FilesystemMetadata> {
        lock(&self.vm.computer.filesystem.shared).take_metadata()
    }

    /// Restores metadata after mounting bytes, before sharing with children.
    pub fn import_filesystem_metadata(&mut self, metadata: FilesystemMetadata) -> Result<()> {
        if Arc::strong_count(&self.vm.computer.filesystem.shared) != 1 {
            bail!("cannot restore filesystem metadata while child processes exist");
        }
        lock(&self.vm.computer.filesystem.shared).import_metadata(metadata)
    }

    /// Returns the recorded exit status, when the guest has exited.
    pub fn exit_status(&self) -> Option<i32> {
        self.exit_status
    }
}

/// Maximum depth of the foreground process stack.
pub const MAX_COMPUTER_PROCESSES: usize = 4;

/// Maximum simultaneously live piped background processes.
pub const MAX_BACKGROUND_PROCESSES: usize = 4;

/// Maximum simultaneously live workspace children.
pub const MAX_WORKSPACE_CHILDREN: usize = 9;

/// A piped background process: no terminal ownership; the parent exchanges
/// bytes with it through the pipe hostcalls.
struct BackgroundChild {
    pid: u32,
    /// Stack depth of the spawning process; only that process may address
    /// this child, and it is reaped when the owner departs.
    owner: usize,
    runtime: ComputerRuntime,
    output: Vec<u8>,
    exit: Option<i32>,
}

/// An independently supervised workspace child sharing the same filesystem.
/// Its terminal endpoint is the parent-held handle instead of the Host terminal.
struct WorkspaceChild {
    handle: u32,
    supervisor: Box<ComputerSupervisor>,
    output: Vec<u8>,
    exit: Option<i32>,
}

/// Supervises computer processes sharing one terminal and `/home`.
///
/// The Host owns every child VM: guests request packages by name, and only
/// packages registered by the Host can be launched. The foreground process
/// (top of the stack) owns terminal input; a parent stays suspended inside
/// its `process_run` hostcall until the child exits. Piped children spawned
/// through `process_spawn` run cooperatively while the parent is suspended
/// inside a pipe or wait hostcall.
pub struct ComputerSupervisor {
    packages: BTreeMap<String, Arc<Vec<u8>>>,
    stack: Vec<ComputerRuntime>,
    background: Vec<BackgroundChild>,
    workspace_children: Vec<WorkspaceChild>,
    environment: Vec<(String, String)>,
    next_pid: u32,
    pending_output: Vec<u8>,
    backend: polkavm::BackendKind,
    max_gas_per_run: u64,
    columns: u32,
    rows: u32,
    network: bool,
    workspace: bool,
    package_resolution: bool,
    pending_resolution: Option<PendingResolution>,
}

/// A spawn suspended awaiting embedder package resolution (open spawn).
enum PendingResolution {
    /// `process_run` from the root foreground.
    Run {
        package: String,
        arguments: Vec<String>,
    },
    /// `process_spawn` (piped) from the root foreground.
    Piped {
        package: String,
        arguments: Vec<String>,
    },
    /// `workspace_spawn` from the root guest.
    Workspace {
        package: String,
        arguments: Vec<String>,
        columns: u32,
        rows: u32,
    },
    /// A workspace child's own supervisor is suspended on a resolution.
    Child { handle: u32 },
}

impl ComputerSupervisor {
    /// Creates a supervisor whose root process runs `program`.
    pub fn new(program: &[u8], context: ComputerContext, max_gas_per_run: u64) -> Result<Self> {
        Self::new_with_backend(
            program,
            context,
            max_gas_per_run,
            crate::preferred_backend(),
        )
    }

    /// Creates a supervisor using an explicitly selected PolkaVM backend.
    pub fn new_with_backend(
        program: &[u8],
        context: ComputerContext,
        max_gas_per_run: u64,
        backend: polkavm::BackendKind,
    ) -> Result<Self> {
        let environment = context.environment.clone();
        let root = ComputerRuntime::new_with_backend(program, context, max_gas_per_run, backend)?;
        Ok(Self {
            packages: BTreeMap::new(),
            stack: vec![root],
            background: Vec::new(),
            workspace_children: Vec::new(),
            next_pid: 2,
            pending_output: Vec::new(),
            environment,
            backend,
            max_gas_per_run,
            columns: 80,
            rows: 24,
            network: false,
            workspace: false,
            package_resolution: false,
            pending_resolution: None,
        })
    }

    /// Enables open package resolution: a spawn naming an unregistered
    /// package suspends the computer with `PackageRequested` instead of
    /// failing with `NOT_FOUND`, so the embedding Host can resolve the
    /// name (e.g. through DotNS), then `provide_package` or
    /// `reject_package`. Disabled by default: the conformance contract
    /// expects immediate `NOT_FOUND`. Workspace children inherit it.
    pub fn set_package_resolution(&mut self, enabled: bool) {
        self.package_resolution = enabled;
    }

    /// Returns the package name awaiting embedder resolution, if any.
    pub fn pending_package(&self) -> Option<String> {
        match self.pending_resolution.as_ref()? {
            PendingResolution::Run { package, .. }
            | PendingResolution::Piped { package, .. }
            | PendingResolution::Workspace { package, .. } => Some(package.clone()),
            PendingResolution::Child { handle } => self
                .workspace_children
                .iter()
                .find(|child| child.handle == *handle)?
                .supervisor
                .pending_package(),
        }
    }

    /// Registers the pending package and retries the suspended spawn.
    pub fn provide_package(&mut self, program: Vec<u8>) -> Result<()> {
        let Some(pending) = self.pending_resolution.take() else {
            bail!("no package resolution is pending");
        };
        match pending {
            PendingResolution::Run { package, arguments } => {
                self.register_package(&package, program)?;
                match self.spawn_child(&package, arguments) {
                    Ok(child) => self.stack.push(child),
                    Err(status) => self.foreground().resolve_spawn(status),
                }
            }
            PendingResolution::Piped { package, arguments } => {
                self.register_package(&package, program)?;
                match self.spawn_piped(&package, arguments) {
                    Ok(pid) => self.foreground().resolve_spawn(pid as i32),
                    Err(status) => self.foreground().resolve_spawn(status),
                }
            }
            PendingResolution::Workspace {
                package,
                arguments,
                columns,
                rows,
            } => {
                self.register_package(&package, program)?;
                match self.spawn_workspace_child(&package, arguments, columns, rows) {
                    Ok(handle) => self.foreground().resolve_spawn(handle as i32),
                    Err(status) => self.foreground().resolve_spawn(status),
                }
            }
            PendingResolution::Child { handle } => {
                // Share the resolution with the whole tree, then route it
                // to the suspended child.
                let Some(index) = self.workspace_index(handle) else {
                    bail!("suspended workspace child is gone");
                };
                if let Some(name) = self.workspace_children[index].supervisor.pending_package() {
                    self.register_package(&name, program.clone())?;
                }
                self.workspace_children[index]
                    .supervisor
                    .provide_package(program)?;
            }
        }
        Ok(())
    }

    /// Fails the suspended spawn; the requesting guest observes `status`.
    pub fn reject_package(&mut self, status: i32) -> Result<()> {
        let Some(pending) = self.pending_resolution.take() else {
            bail!("no package resolution is pending");
        };
        match pending {
            PendingResolution::Run { .. }
            | PendingResolution::Piped { .. }
            | PendingResolution::Workspace { .. } => {
                self.foreground().resolve_spawn(status);
            }
            PendingResolution::Child { handle } => {
                let Some(index) = self.workspace_index(handle) else {
                    bail!("suspended workspace child is gone");
                };
                self.workspace_children[index]
                    .supervisor
                    .reject_package(status)?;
            }
        }
        Ok(())
    }

    /// Registers a launchable package under a Host-authorized name.
    pub fn register_package(&mut self, name: &str, program: Vec<u8>) -> Result<()> {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            bail!("invalid package name {name:?}");
        }
        self.packages.insert(name.to_owned(), Arc::new(program));
        Ok(())
    }

    /// Returns the selected execution backend.
    pub fn backend(&self) -> polkavm::BackendKind {
        self.backend
    }

    /// Mounts one persistent file into the authoritative tree-wide `/home` store.
    /// Open destinations are rejected without changing any process's view.
    pub fn mount_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.foreground().mount_file(path, bytes)
    }

    /// Sets the terminal size observed by every process.
    pub fn set_terminal_size(&mut self, columns: u32, rows: u32) -> Result<()> {
        for process in &mut self.stack {
            process.set_terminal_size(columns, rows)?;
        }
        for child in &mut self.background {
            child.runtime.set_terminal_size(columns, rows)?;
        }
        self.columns = columns;
        self.rows = rows;
        Ok(())
    }

    /// Grants or revokes outbound TCP for every current and future process.
    pub fn set_network_enabled(&mut self, enabled: bool) {
        for process in &mut self.stack {
            process.set_network_enabled(enabled);
        }
        for child in &mut self.background {
            child.runtime.set_network_enabled(enabled);
        }
        for child in &mut self.workspace_children {
            child.supervisor.set_network_enabled(enabled);
        }
        self.network = enabled;
    }

    /// Grants or revokes the workspace capability for the root process.
    /// Revocation reaps every live workspace child.
    pub fn set_workspace_enabled(&mut self, enabled: bool) {
        self.workspace = enabled;
        if !enabled {
            self.workspace_children.clear();
            match self.pending_resolution {
                Some(PendingResolution::Workspace { .. }) => {
                    self.pending_resolution = None;
                    self.foreground().resolve_spawn(STATUS_DENIED);
                }
                Some(PendingResolution::Child { .. }) => self.pending_resolution = None,
                _ => {}
            }
        }
    }

    /// Queues keyboard bytes toward the foreground process.
    pub fn send_terminal_input(&mut self, bytes: &[u8]) -> Result<()> {
        self.foreground().send_terminal_input(bytes)
    }

    /// Returns remaining space in the foreground process's input queue.
    pub fn terminal_input_space(&self) -> usize {
        self.stack
            .last()
            .map_or(0, ComputerRuntime::terminal_input_space)
    }
    /// Drains ANSI output: first any output a departed child left behind,
    /// then the foreground process's buffer.
    pub fn take_terminal_output(&mut self) -> Option<Vec<u8>> {
        if !self.pending_output.is_empty() {
            return Some(core::mem::take(&mut self.pending_output));
        }
        self.foreground().take_terminal_output()
    }

    /// Returns whether undelivered terminal input remains queued.
    pub fn has_terminal_input(&self) -> bool {
        self.stack
            .last()
            .is_some_and(ComputerRuntime::has_terminal_input)
    }

    /// Drains files modified by any process since the previous call.
    pub fn take_modified_files(&mut self) -> Vec<(String, Vec<u8>)> {
        self.foreground().take_modified_files()
    }

    /// Drains paths removed by any process since the previous call.
    pub fn take_removed_files(&mut self) -> Vec<String> {
        self.foreground().take_removed_files()
    }

    /// Exports the whole shared namespace with stable identities and times.
    pub fn export_filesystem_metadata(&self) -> FilesystemMetadata {
        self.stack[0].export_filesystem_metadata()
    }

    /// Drains metadata changes from the same store as both byte delta drains.
    pub fn take_filesystem_metadata(&mut self) -> Option<FilesystemMetadata> {
        self.foreground().take_filesystem_metadata()
    }

    /// Restores mounted metadata before any child or open file exists.
    pub fn import_filesystem_metadata(&mut self, metadata: FilesystemMetadata) -> Result<()> {
        self.stack[0].import_filesystem_metadata(metadata)
    }

    /// Exit status reported for a child that faulted (trap, gas, segfault).
    const FAULTED_CHILD_STATUS: i32 = 139;

    /// Maximum contained child faults per `run()` before erroring out.
    const MAX_FAULT_POPS_PER_RUN: usize = 32;

    /// Runs the foreground process until the system yields or the root exits.
    ///
    /// A fault in a child process (trap, out of gas) fails only that child:
    /// it is discarded and its parent resumes with status 139. Only a root
    /// fault propagates as an error.
    pub fn run(&mut self) -> Result<ComputerStatus> {
        let result = self.run_inner();
        if result.is_err() {
            for process in &mut self.stack {
                process.dispose();
            }
            self.background.clear();
            self.workspace_children.clear();
            self.pending_resolution = None;
        }
        result
    }

    fn run_inner(&mut self) -> Result<ComputerStatus> {
        if self.pending_resolution.is_some() {
            // Idempotent while suspended: the embedder must provide or
            // reject the pending package before execution continues.
            return Ok(ComputerStatus::PackageRequested);
        }
        // Bound the fault-containment path so a root that spawns
        // immediately-faulting children in a loop cannot keep run() from
        // returning control to the Host.
        let mut fault_pops = 0usize;
        loop {
            let status = match self.foreground().run() {
                Ok(status) => status,
                Err(error) => {
                    if self.stack.len() == 1 {
                        self.background.clear();
                        self.workspace_children.clear();
                        return Err(error);
                    }
                    fault_pops += 1;
                    if fault_pops > Self::MAX_FAULT_POPS_PER_RUN {
                        for process in &mut self.stack {
                            process.dispose();
                        }
                        self.background.clear();
                        self.workspace_children.clear();
                        return Err(error.context("children faulted repeatedly"));
                    }
                    self.pop_foreground(Self::FAULTED_CHILD_STATUS)?;
                    continue;
                }
            };
            match status {
                ComputerStatus::Yielded => {
                    // Surface a suspended workspace child's resolution once
                    // the workspace guest has yielded; the embedder resolves
                    // it before execution continues anywhere in the tree.
                    if let Some(child) = self.workspace_children.iter().find(|child| {
                        child.exit.is_none() && child.supervisor.pending_package().is_some()
                    }) {
                        self.pending_resolution = Some(PendingResolution::Child {
                            handle: child.handle,
                        });
                        return Ok(ComputerStatus::PackageRequested);
                    }
                    return Ok(ComputerStatus::Yielded);
                }
                ComputerStatus::SpawnRequested => {
                    let request = self.foreground().take_spawn_request();
                    let Some((package, arguments)) = request else {
                        bail!("spawn status without a pending request");
                    };
                    if self.package_resolution && !self.packages.contains_key(&package) {
                        self.pending_resolution =
                            Some(PendingResolution::Run { package, arguments });
                        return Ok(ComputerStatus::PackageRequested);
                    }
                    match self.spawn_child(&package, arguments) {
                        Ok(child) => self.stack.push(child),
                        Err(status) => self.foreground().resolve_spawn(status),
                    }
                }
                ComputerStatus::ChildRequest => {
                    let request = self.foreground().take_child_request();
                    let Some(request) = request else {
                        bail!("child-request status without a pending request");
                    };
                    self.handle_child_request(request)?;
                    if self.pending_resolution.is_some() {
                        return Ok(ComputerStatus::PackageRequested);
                    }
                }
                ComputerStatus::Exited(code) => {
                    // The exited root stays resident so terminal accessors
                    // remain valid; rerunning it reports the same status.
                    if self.stack.len() == 1 {
                        self.background.clear();
                        self.workspace_children.clear();
                        return Ok(ComputerStatus::Exited(code));
                    }
                    // Mask to the POSIX exit-code byte: negative statuses
                    // stay failures (e.g. -1 -> 255) and cannot alias the
                    // negative hostcall error space.
                    self.pop_foreground(code & 0xff)?;
                }
                ComputerStatus::PackageRequested => {
                    bail!("a process runtime cannot request package resolution")
                }
            }
        }
    }

    /// Discards the foreground child: forwards its remaining terminal
    /// output, reaps its orphaned background children, and resolves the parent.
    fn pop_foreground(&mut self, status: i32) -> Result<()> {
        debug_assert!(
            self.stack.len() >= 2,
            "pop_foreground requires a child on the stack"
        );
        let child = self.stack.pop();
        // Preserve terminal write order: the parent's bytes written before
        // the spawn precede the child's remaining output.
        if let Some(bytes) = self.foreground().take_terminal_output() {
            let available = MAX_TTY_OUTPUT_BYTES.saturating_sub(self.pending_output.len());
            self.pending_output
                .extend_from_slice(&bytes[..bytes.len().min(available)]);
        }
        if let Some(mut child) = child {
            if let Some(bytes) = child.take_terminal_output() {
                let available = MAX_TTY_OUTPUT_BYTES.saturating_sub(self.pending_output.len());
                self.pending_output
                    .extend_from_slice(&bytes[..bytes.len().min(available)]);
            }
        }
        // Background children spawned by the departed process (or anything
        // deeper) are unreachable now; reap them so their slots, memory,
        // and sockets do not leak.
        let depth = self.stack.len();
        self.background.retain(|child| child.owner <= depth);
        self.foreground().resolve_spawn(status);
        Ok(())
    }

    /// Host-authority cancellation of the foreground process.
    ///
    /// A child is discarded and its parent resumes with status 130
    /// (interrupted). Terminating the root ends the whole computer.
    pub fn terminate_foreground(&mut self) -> Result<ComputerStatus> {
        if self.stack.len() == 1 {
            // Record the exit so subsequent run() calls stay terminated; a
            // root that already exited keeps its genuine status.
            let status = *self.foreground().exit_status.get_or_insert(130);
            self.foreground().dispose();
            self.background.clear();
            self.workspace_children.clear();
            self.pending_resolution = None;
            return Ok(ComputerStatus::Exited(status));
        }
        self.pending_resolution = None;
        self.pop_foreground(130)?;
        Ok(ComputerStatus::Yielded)
    }

    fn foreground(&mut self) -> &mut ComputerRuntime {
        self.stack
            .last_mut()
            .expect("supervisor stack is never empty")
    }

    /// Executes one spawn/wait/pipe request and resolves it into the caller.
    fn handle_child_request(&mut self, request: ChildProcessRequest) -> Result<()> {
        match request {
            ChildProcessRequest::Spawn { package, arguments } => {
                if self.background.len() >= MAX_BACKGROUND_PROCESSES {
                    self.foreground().resolve_spawn(STATUS_LIMIT);
                    return Ok(());
                }
                if self.package_resolution && !self.packages.contains_key(&package) {
                    self.pending_resolution = Some(PendingResolution::Piped { package, arguments });
                    return Ok(());
                }
                match self.spawn_piped(&package, arguments) {
                    Ok(pid) => self.foreground().resolve_spawn(pid as i32),
                    Err(status) => self.foreground().resolve_spawn(status),
                }
            }
            ChildProcessRequest::Wait { pid } => {
                let Some(index) = self.background_index(pid) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                self.drive_background(index)?;
                match self.background[index].exit {
                    Some(status) => {
                        self.reap_background(index)?;
                        self.foreground().resolve_spawn(status & 0xff);
                    }
                    None => self.foreground().resolve_spawn(STATUS_WOULD_BLOCK),
                }
            }
            ChildProcessRequest::PipeWrite { pid, bytes } => {
                let Some(index) = self.background_index(pid) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                let child = &mut self.background[index];
                if child.exit.is_some() || child.runtime.terminal_input_closed() {
                    self.foreground().resolve_spawn(STATUS_INVALID);
                    return Ok(());
                }
                let space = child.runtime.terminal_input_space();
                let written = bytes.len().min(space);
                if written > 0 {
                    child.runtime.send_terminal_input(&bytes[..written])?;
                }
                self.drive_background(index)?;
                self.foreground().resolve_spawn(written as i32);
            }
            ChildProcessRequest::PipeRead {
                pid,
                destination,
                capacity,
            } => {
                let Some(index) = self.background_index(pid) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                if self.background[index].output.is_empty() {
                    self.drive_background(index)?;
                }
                let child = &mut self.background[index];
                if !child.output.is_empty() {
                    let count = child.output.len().min(capacity);
                    let bytes: Vec<u8> = child.output.drain(..count).collect();
                    self.foreground().resolve_read(destination, &bytes)?;
                } else if child.exit.is_some() {
                    self.foreground().resolve_spawn(0);
                } else {
                    self.foreground().resolve_spawn(STATUS_WOULD_BLOCK);
                }
            }
            ChildProcessRequest::PipeClose { pid } => {
                let Some(index) = self.background_index(pid) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                self.background[index].runtime.close_terminal_input();
                self.drive_background(index)?;
                self.foreground().resolve_spawn(0);
            }
            request @ (ChildProcessRequest::WorkspaceSpawn { .. }
            | ChildProcessRequest::WorkspaceSendInput { .. }
            | ChildProcessRequest::WorkspaceRead { .. }
            | ChildProcessRequest::WorkspaceResize { .. }
            | ChildProcessRequest::WorkspaceWait { .. }
            | ChildProcessRequest::WorkspaceClose { .. }) => {
                // Only the root computer holding the workspace grant may
                // manage children; nested computers are never granted it.
                if !self.workspace || self.stack.len() != 1 {
                    self.foreground().resolve_spawn(STATUS_DENIED);
                    return Ok(());
                }
                self.handle_workspace_request(request)?;
            }
        }
        Ok(())
    }

    /// Executes one workspace operation and resolves it into the root guest.
    fn handle_workspace_request(&mut self, request: ChildProcessRequest) -> Result<()> {
        match request {
            ChildProcessRequest::WorkspaceSpawn {
                package,
                arguments,
                columns,
                rows,
            } => {
                if self.workspace_children.len() >= MAX_WORKSPACE_CHILDREN {
                    self.foreground().resolve_spawn(STATUS_LIMIT);
                    return Ok(());
                }
                if self.package_resolution && !self.packages.contains_key(&package) {
                    self.pending_resolution = Some(PendingResolution::Workspace {
                        package,
                        arguments,
                        columns,
                        rows,
                    });
                    return Ok(());
                }
                match self.spawn_workspace_child(&package, arguments, columns, rows) {
                    Ok(handle) => self.foreground().resolve_spawn(handle as i32),
                    Err(status) => self.foreground().resolve_spawn(status),
                }
            }
            ChildProcessRequest::WorkspaceSendInput { handle, bytes } => {
                let Some(index) = self.workspace_index(handle) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                let child = &mut self.workspace_children[index];
                if child.exit.is_some() {
                    self.foreground().resolve_spawn(STATUS_INVALID);
                    return Ok(());
                }
                let space = child.supervisor.terminal_input_space();
                let written = bytes.len().min(space);
                if written > 0 {
                    child.supervisor.send_terminal_input(&bytes[..written])?;
                }
                self.drive_workspace_child(index)?;
                self.foreground().resolve_spawn(written as i32);
            }
            ChildProcessRequest::WorkspaceRead {
                handle,
                destination,
                capacity,
            } => {
                let Some(index) = self.workspace_index(handle) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                if self.workspace_children[index].output.is_empty() {
                    self.drive_workspace_child(index)?;
                }
                let child = &mut self.workspace_children[index];
                if !child.output.is_empty() {
                    let count = child.output.len().min(capacity);
                    let bytes: Vec<u8> = child.output.drain(..count).collect();
                    self.foreground().resolve_read(destination, &bytes)?;
                } else if child.exit.is_some() {
                    self.foreground().resolve_spawn(0);
                } else {
                    self.foreground().resolve_spawn(STATUS_WOULD_BLOCK);
                }
            }
            ChildProcessRequest::WorkspaceResize {
                handle,
                columns,
                rows,
            } => {
                let Some(index) = self.workspace_index(handle) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                let child = &mut self.workspace_children[index];
                if child.exit.is_some()
                    || child.supervisor.set_terminal_size(columns, rows).is_err()
                {
                    self.foreground().resolve_spawn(STATUS_INVALID);
                    return Ok(());
                }
                self.foreground().resolve_spawn(0);
            }
            ChildProcessRequest::WorkspaceWait { handle } => {
                let Some(index) = self.workspace_index(handle) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                self.drive_workspace_child(index)?;
                // The handle stays valid after exit so remaining output can
                // be drained; workspace_close reclaims the slot.
                match self.workspace_children[index].exit {
                    Some(status) => self.foreground().resolve_spawn(status & 0xff),
                    None => self.foreground().resolve_spawn(STATUS_WOULD_BLOCK),
                }
            }
            ChildProcessRequest::WorkspaceClose { handle } => {
                let Some(index) = self.workspace_index(handle) else {
                    self.foreground().resolve_spawn(STATUS_BAD_HANDLE);
                    return Ok(());
                };
                // Shared writes remain visible; dropping the child releases its handles.
                self.workspace_children.remove(index);
                self.foreground().resolve_spawn(0);
            }
            request => bail!("non-workspace request {request:?} routed to workspace handler"),
        }
        Ok(())
    }

    /// Resolves a workspace child handle to its slot.
    fn workspace_index(&self, handle: u32) -> Option<usize> {
        self.workspace_children
            .iter()
            .position(|child| child.handle == handle)
    }

    /// Runs a workspace child until it exits or blocks awaiting input,
    /// collecting its terminal output. Files are already shared.
    ///
    /// Cooperative scheduling: workspace children only execute while the
    /// workspace guest is suspended inside a workspace hostcall.
    fn drive_workspace_child(&mut self, index: usize) -> Result<()> {
        const MAX_DRIVE_STEPS: usize = 64;
        for _ in 0..MAX_DRIVE_STEPS {
            let child = &mut self.workspace_children[index];
            if child.exit.is_some() {
                return Ok(());
            }
            // A faulted child fails alone; the workspace observes the fault
            // status through wait. Its final output and writes still land.
            let outcome = child.supervisor.run();
            while let Some(bytes) = child.supervisor.take_terminal_output() {
                let available = MAX_TTY_OUTPUT_BYTES.saturating_sub(child.output.len());
                child
                    .output
                    .extend_from_slice(&bytes[..bytes.len().min(available)]);
            }
            let exit = match outcome {
                Ok(ComputerStatus::Exited(code)) => Some(code & 0xff),
                Ok(ComputerStatus::Yielded) => None,
                // A suspended package resolution surfaces at the parent's
                // next Yielded return; stop driving without progress.
                Ok(ComputerStatus::PackageRequested) => return Ok(()),
                // A nested supervisor only surfaces Yielded or Exited.
                Ok(_) | Err(_) => Some(Self::FAULTED_CHILD_STATUS),
            };
            let child = &mut self.workspace_children[index];
            if let Some(code) = exit {
                for process in &mut child.supervisor.stack {
                    process.dispose();
                }
                child.supervisor.background.clear();
                child.supervisor.workspace_children.clear();
                child.exit = Some(code);
                return Ok(());
            }
            if !child.supervisor.has_terminal_input() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Launches an independently supervised workspace child.
    fn spawn_workspace_child(
        &mut self,
        package: &str,
        arguments: Vec<String>,
        columns: u32,
        rows: u32,
    ) -> Result<u32, i32> {
        if !(1..=1_000).contains(&columns) || !(1..=1_000).contains(&rows) {
            return Err(STATUS_INVALID);
        }
        let Some(program) = self.packages.get(package).cloned() else {
            return Err(STATUS_NOT_FOUND);
        };
        let mut argv = Vec::with_capacity(arguments.len() + 1);
        argv.push(package.to_owned());
        argv.extend(arguments);
        let context =
            ComputerContext::new(argv, self.environment.clone()).map_err(|_| STATUS_INVALID)?;
        let mut child = ComputerSupervisor::new_with_backend(
            &program,
            context,
            self.max_gas_per_run,
            self.backend,
        )
        .map_err(|_| STATUS_INVALID)?;
        // The nested computer shares the Host-authorized package registry so
        // a shell pane can run editors; it is never granted host.workspace.
        child.packages = self.packages.clone();
        child
            .set_terminal_size(columns, rows)
            .map_err(|_| STATUS_INVALID)?;
        child.set_network_enabled(self.network);
        child.package_resolution = self.package_resolution;
        let shared = self.stack[0].vm.computer.filesystem.shared.clone();
        child.stack[0].vm.computer.filesystem.share(shared);
        let handle = self.next_pid;
        self.next_pid += 1;
        self.workspace_children.push(WorkspaceChild {
            handle,
            supervisor: Box::new(child),
            output: Vec::new(),
            exit: None,
        });
        Ok(handle)
    }

    /// Resolves a pid to a background slot, enforcing ownership: only the
    /// process that spawned a child may address it.
    fn background_index(&self, pid: u32) -> Option<usize> {
        let depth = self.stack.len();
        self.background
            .iter()
            .position(|child| child.pid == pid && child.owner == depth)
    }

    /// Runs a background child until it exits or blocks awaiting input.
    ///
    /// Cooperative scheduling: background children only execute while the
    /// foreground process is suspended inside a pipe or wait hostcall.
    fn drive_background(&mut self, index: usize) -> Result<()> {
        const MAX_DRIVE_STEPS: usize = 1_024;
        for _ in 0..MAX_DRIVE_STEPS {
            let child = &mut self.background[index];
            if child.exit.is_some() {
                return Ok(());
            }
            // A faulted piped child fails alone; the parent observes the
            // fault status through wait. Shared writes remain visible and
            // its final terminal output is collected below.
            let outcome = child.runtime.run().ok();
            if let Some(bytes) = child.runtime.take_terminal_output() {
                let available = MAX_TTY_OUTPUT_BYTES.saturating_sub(child.output.len());
                child
                    .output
                    .extend_from_slice(&bytes[..bytes.len().min(available)]);
            }
            let child = &mut self.background[index];
            let Some(status) = outcome else {
                child.exit = Some(Self::FAULTED_CHILD_STATUS);
                return Ok(());
            };
            match status {
                ComputerStatus::Exited(code) => {
                    child.exit = Some(code & 0xff);
                    return Ok(());
                }
                ComputerStatus::Yielded => {
                    if !child.runtime.has_terminal_input() {
                        return Ok(());
                    }
                }
                ComputerStatus::SpawnRequested => {
                    // Background children cannot own the terminal.
                    let _ = child.runtime.take_spawn_request();
                    child.runtime.resolve_spawn(STATUS_DENIED);
                }
                ComputerStatus::ChildRequest => {
                    // No nested background trees in the experimental contract.
                    let _ = child.runtime.take_child_request();
                    child.runtime.resolve_spawn(STATUS_DENIED);
                }
                ComputerStatus::PackageRequested => {
                    // Process runtimes never request package resolution;
                    // treat a stray status as a contained child fault.
                    child.exit = Some(Self::FAULTED_CHILD_STATUS);
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Reclaims an exited background child and its process-local resources.
    fn reap_background(&mut self, index: usize) -> Result<()> {
        self.background.remove(index);
        Ok(())
    }

    /// Launches a piped background child and returns its pid.
    fn spawn_piped(&mut self, package: &str, arguments: Vec<String>) -> Result<u32, i32> {
        let child = self.spawn_child(package, arguments)?;
        let pid = self.next_pid;
        self.next_pid += 1;
        self.background.push(BackgroundChild {
            pid,
            owner: self.stack.len(),
            runtime: child,
            output: Vec::new(),
            exit: None,
        });
        Ok(pid)
    }

    fn spawn_child(
        &mut self,
        package: &str,
        arguments: Vec<String>,
    ) -> Result<ComputerRuntime, i32> {
        if self.stack.len() >= MAX_COMPUTER_PROCESSES {
            return Err(STATUS_LIMIT);
        }
        let Some(program) = self.packages.get(package) else {
            return Err(STATUS_NOT_FOUND);
        };
        let mut argv = Vec::with_capacity(arguments.len() + 1);
        argv.push(package.to_owned());
        argv.extend(arguments);
        // Children inherit the computer's launch environment.
        let context =
            ComputerContext::new(argv, self.environment.clone()).map_err(|_| STATUS_INVALID)?;
        let mut child =
            ComputerRuntime::new_with_backend(program, context, self.max_gas_per_run, self.backend)
                .map_err(|_| STATUS_INVALID)?;
        child
            .set_terminal_size(self.columns, self.rows)
            .map_err(|_| STATUS_INVALID)?;
        child.set_network_enabled(self.network);
        child
            .vm
            .computer
            .filesystem
            .share(self.stack[0].vm.computer.filesystem.shared.clone());
        Ok(child)
    }
}

fn encode_arguments(arguments: &[String]) -> Result<Vec<u8>> {
    let mut output = encoded_record(arguments.len())?;
    for argument in arguments {
        push_bytes(&mut output, argument.as_bytes())?;
    }
    Ok(output)
}

fn encode_environment(environment: &[(String, String)]) -> Result<Vec<u8>> {
    let mut output = encoded_record(environment.len())?;
    for (key, value) in environment {
        push_bytes(&mut output, key.as_bytes())?;
        push_bytes(&mut output, value.as_bytes())?;
    }
    Ok(output)
}

fn encoded_record(count: usize) -> Result<Vec<u8>> {
    let count = u32::try_from(count).context("computer context entry count overflow")?;
    Ok(count.to_le_bytes().to_vec())
}

fn push_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let length = u32::try_from(bytes.len()).context("computer context field length overflow")?;
    let required = output
        .len()
        .checked_add(4)
        .and_then(|length| length.checked_add(bytes.len()))
        .ok_or_else(|| anyhow!("computer context length overflow"))?;
    if required > MAX_COMPUTER_CONTEXT_BYTES {
        bail!("encoded computer context exceeds the host limit");
    }
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXITING_GUEST: &[u8] = include_bytes!("../tests/fixtures/computer-core-services.polkavm");

    fn filesystem_supervisor() -> ComputerSupervisor {
        let mut supervisor = ComputerSupervisor::new_with_backend(
            EXITING_GUEST,
            ComputerContext::default(),
            50_000_000,
            polkavm::BackendKind::Interpreter,
        )
        .unwrap();
        supervisor
            .register_package("child", EXITING_GUEST.to_vec())
            .unwrap();
        supervisor
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn workspace_network_revocation_closes_streams_and_denies_future_children() {
        use std::io::Read;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let mut supervisor = filesystem_supervisor();
        supervisor.set_workspace_enabled(true);
        supervisor.set_network_enabled(true);
        let handle = supervisor
            .spawn_workspace_child("child", vec![], 80, 24)
            .unwrap();
        let index = supervisor.workspace_index(handle).unwrap();
        let socket = supervisor.workspace_children[index]
            .supervisor
            .foreground()
            .vm
            .computer
            .net_tcp_connect(&address);
        assert!(socket >= FIRST_SOCKET_HANDLE as i32);
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();

        supervisor.set_network_enabled(false);
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        let pane = &mut supervisor.workspace_children[index].supervisor;
        assert_eq!(
            pane.foreground().vm.computer.net_tcp_connect(&address),
            STATUS_DENIED
        );
        assert_eq!(
            pane.foreground().vm.computer.net_write(socket as u32, b"x"),
            STATUS_BAD_HANDLE
        );
        let mut nested = pane.spawn_child("child", vec![]).unwrap();
        assert_eq!(nested.vm.computer.net_tcp_connect(&address), STATUS_DENIED);
    }

    #[test]
    fn workspace_exit_retains_nested_output_and_the_panes_final_reply() {
        let mut supervisor = filesystem_supervisor();
        for (name, bytes) in [
            (
                "pane",
                include_bytes!("../tests/fixtures/computer-workspace-pane.polkavm").as_slice(),
            ),
            (
                "extra",
                include_bytes!("../tests/fixtures/computer-pipe-driver.polkavm").as_slice(),
            ),
            (
                "upper",
                include_bytes!("../tests/fixtures/computer-pipe-filter.polkavm").as_slice(),
            ),
        ] {
            supervisor.register_package(name, bytes.to_vec()).unwrap();
        }
        let handle = supervisor
            .spawn_workspace_child("pane", vec![], 80, 24)
            .unwrap();
        let index = supervisor.workspace_index(handle).unwrap();
        supervisor.workspace_children[index]
            .supervisor
            .send_terminal_input(b"pq")
            .unwrap();
        supervisor.drive_workspace_child(index).unwrap();
        let child = &supervisor.workspace_children[index];
        assert_eq!(child.exit, Some(7));
        assert_eq!(child.output.as_slice(), b"pane:readyHELLO, PIPESp:0");
    }

    #[test]
    fn cancelling_nested_resolution_preserves_the_interrupted_result() {
        let mut supervisor = ComputerSupervisor::new_with_backend(
            include_bytes!("../tests/fixtures/computer-workspace-pane.polkavm"),
            ComputerContext::default(),
            50_000_000,
            polkavm::BackendKind::Interpreter,
        )
        .unwrap();
        supervisor.set_package_resolution(true);
        supervisor.send_terminal_input(b"p").unwrap();
        assert_eq!(supervisor.run().unwrap(), ComputerStatus::PackageRequested);
        supervisor
            .provide_package(
                include_bytes!("../tests/fixtures/computer-pipe-driver.polkavm").to_vec(),
            )
            .unwrap();
        assert_eq!(supervisor.run().unwrap(), ComputerStatus::PackageRequested);
        assert_eq!(
            supervisor.terminate_foreground().unwrap(),
            ComputerStatus::Yielded
        );
        assert_eq!(supervisor.pending_package(), None);
        assert!(supervisor.provide_package(EXITING_GUEST.to_vec()).is_err());
        assert_eq!(supervisor.run().unwrap(), ComputerStatus::Yielded);
        let mut output = Vec::new();
        while let Some(bytes) = supervisor.take_terminal_output() {
            output.extend(bytes);
        }
        assert!(output.ends_with(b"p:130"));
    }

    #[test]
    fn cancelled_foreground_preserves_shared_writes_and_releases_open_paths() {
        let mut supervisor = filesystem_supervisor();
        supervisor
            .mount_file("/home/dest", b"old".to_vec())
            .unwrap();
        let child = supervisor.spawn_child("child", vec![]).unwrap();
        supervisor.stack.push(child);
        let handle = supervisor.foreground().vm.computer.fs_open(
            "/home/lock",
            FS_OPEN_WRITE | FS_OPEN_CREATE | FS_OPEN_EXCLUSIVE,
        ) as u32;
        assert_eq!(
            supervisor.foreground().vm.computer.fs_write(handle, b"new"),
            3
        );
        assert_eq!(
            supervisor.stack[0]
                .vm
                .computer
                .filesystem
                .rename("/home/lock", "/home/dest"),
            STATUS_DENIED
        );
        assert_eq!(
            supervisor.terminate_foreground().unwrap(),
            ComputerStatus::Yielded
        );
        assert_eq!(
            supervisor
                .foreground()
                .vm
                .computer
                .filesystem
                .rename("/home/lock", "/home/dest"),
            0
        );
        assert_eq!(
            supervisor.take_modified_files(),
            vec![("/home/dest".into(), b"new".to_vec())]
        );
        assert_eq!(supervisor.take_removed_files(), vec!["/home/lock"]);
        assert!(supervisor.take_filesystem_metadata().is_some());
        assert!(supervisor.take_filesystem_metadata().is_none());
    }

    #[test]
    fn piped_and_workspace_processes_observe_one_store_without_scheduling_drains() {
        let mut supervisor = filesystem_supervisor();
        supervisor.spawn_piped("child", vec![]).unwrap();
        let workspace = supervisor
            .spawn_workspace_child("child", vec![], 80, 24)
            .unwrap();
        let flags = FS_OPEN_WRITE | FS_OPEN_CREATE | FS_OPEN_APPEND;
        let piped_handle = supervisor.background[0]
            .runtime
            .vm
            .computer
            .fs_open("/home/log", flags) as u32;
        assert_eq!(
            supervisor.background[0]
                .runtime
                .vm
                .computer
                .fs_write(piped_handle, b"pipe"),
            4
        );
        let pane = &mut supervisor.workspace_children[0].supervisor.stack[0]
            .vm
            .computer;
        assert_eq!(pane.fs_stat("/home/log"), Some(4));
        let pane_handle = pane.fs_open("/home/log", flags) as u32;
        assert_eq!(pane.fs_write(pane_handle, b"pane"), 4);
        supervisor.drive_background(0).unwrap();
        assert_eq!(
            supervisor.take_modified_files(),
            vec![("/home/log".into(), b"pipepane".to_vec())]
        );
        assert_eq!(
            supervisor.foreground().vm.computer.fs_remove("/home/log"),
            STATUS_DENIED
        );
        supervisor
            .handle_workspace_request(ChildProcessRequest::WorkspaceClose { handle: workspace })
            .unwrap();
        // The exited background runtime is retained for wait/output, but owns no open paths.
        assert_eq!(
            supervisor.foreground().vm.computer.fs_remove("/home/log"),
            0
        );
        assert_eq!(supervisor.take_removed_files(), vec!["/home/log"]);
    }

    #[test]
    fn retained_exited_and_faulted_runtimes_release_their_file_handles() {
        for gas in [1, 50_000_000] {
            let mut runtime = ComputerRuntime::new_with_backend(
                EXITING_GUEST,
                ComputerContext::default(),
                gas,
                polkavm::BackendKind::Interpreter,
            )
            .unwrap();
            let handle = runtime
                .vm
                .computer
                .fs_open("/home/open", FS_OPEN_WRITE | FS_OPEN_CREATE)
                as u32;
            assert_eq!(runtime.vm.computer.fs_write(handle, b"kept"), 4);
            let mut observer = FileSession::new();
            observer.share(runtime.vm.computer.filesystem.shared.clone());
            assert_eq!(observer.remove("/home/open"), STATUS_DENIED);
            if gas == 1 {
                assert!(runtime.run().is_err());
            } else {
                assert_eq!(runtime.run().unwrap(), ComputerStatus::Exited(31));
            }
            assert_eq!(
                runtime.take_modified_files(),
                vec![("/home/open".into(), b"kept".to_vec())]
            );
            assert_eq!(observer.remove("/home/open"), 0);
        }
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn terminal_paths_close_native_network_streams() {
        use std::io::Read;

        for terminal in ["exit", "fault", "cancel"] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let mut supervisor = ComputerSupervisor::new_with_backend(
                include_bytes!("../tests/fixtures/computer-core-services.polkavm"),
                ComputerContext::default(),
                if terminal == "fault" { 1 } else { 50_000_000 },
                polkavm::BackendKind::Interpreter,
            )
            .unwrap();
            supervisor.set_network_enabled(true);
            assert!(
                supervisor
                    .foreground()
                    .vm
                    .computer
                    .net_tcp_connect(&listener.local_addr().unwrap().to_string())
                    > 0
            );
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(std::time::Duration::from_secs(1)))
                .unwrap();
            match terminal {
                "exit" => assert_eq!(supervisor.run().unwrap(), ComputerStatus::Exited(31)),
                "fault" => assert!(supervisor.run().is_err()),
                _ => assert_eq!(
                    supervisor.terminate_foreground().unwrap(),
                    ComputerStatus::Exited(130)
                ),
            }
            assert_eq!(peer.read(&mut [0]).unwrap(), 0, "{terminal}");
        }
    }

    #[test]
    fn context_encoding_is_length_delimited_and_ordered() {
        let context = ComputerContext::new(
            vec!["shell.polkavm".into(), "--login".into()],
            vec![
                ("HOME".into(), "/home".into()),
                ("TERM".into(), "pvm-tty".into()),
            ],
        )
        .unwrap();

        assert_eq!(
            context.encoded_arguments,
            b"\x02\0\0\0\x0d\0\0\0shell.polkavm\x07\0\0\0--login"
        );
        assert_eq!(
            context.encoded_environment,
            b"\x02\0\0\0\x04\0\0\0HOME\x05\0\0\0/home\x04\0\0\0TERM\x07\0\0\0pvm-tty"
        );
    }

    #[test]
    fn context_rejects_ambiguous_environment() {
        assert!(ComputerContext::new(Vec::new(), vec![("".into(), "value".into())]).is_err());
        assert!(ComputerContext::new(Vec::new(), vec![("A=B".into(), "value".into())]).is_err());
        assert!(ComputerContext::new(
            Vec::new(),
            vec![("HOME".into(), "one".into()), ("HOME".into(), "two".into())]
        )
        .is_err());
    }

    #[test]
    fn terminal_reads_block_until_input_arrives() {
        let mut devices = ComputerDevices::new();
        let mut buffer = [0u8; 4];
        assert_eq!(
            devices.tty_read_into(COMPUTER_TTY_HANDLE, &mut buffer),
            STATUS_WOULD_BLOCK
        );
        devices.push_terminal_input(b"hi").unwrap();
        assert_eq!(devices.tty_read_into(COMPUTER_TTY_HANDLE, &mut buffer), 2);
        assert_eq!(&buffer[..2], b"hi");
        assert_eq!(devices.tty_read_into(2, &mut buffer), STATUS_BAD_HANDLE);
    }

    #[test]
    fn terminal_output_is_drained_by_the_host() {
        let mut devices = ComputerDevices::new();
        assert_eq!(devices.tty_write(COMPUTER_TTY_HANDLE, b"\x1b[2J"), 4);
        assert_eq!(
            devices.take_terminal_output().as_deref(),
            Some(b"\x1b[2J".as_slice())
        );
        assert!(devices.take_terminal_output().is_none());
    }

    #[test]
    fn files_create_write_seek_read_and_track_modification() {
        let mut devices = ComputerDevices::new();
        assert_eq!(
            devices.fs_open("/home/hello.c", FS_OPEN_READ),
            STATUS_NOT_FOUND
        );
        let handle = devices.fs_open(
            "/home/hello.c",
            FS_OPEN_READ | FS_OPEN_WRITE | FS_OPEN_CREATE,
        );
        assert!(handle > 0);
        let handle = handle as u32;
        assert_eq!(devices.fs_write(handle, b"hello world"), 11);
        assert_eq!(devices.fs_truncate(handle, 5), 0);
        assert_eq!(devices.fs_seek(handle, 0, 0), 0);
        let mut buffer = [0u8; 16];
        assert_eq!(devices.fs_read(handle, &mut buffer), 5);
        assert_eq!(&buffer[..5], b"hello");
        assert_eq!(devices.fs_stat("/home/hello.c"), Some(5));
        assert_eq!(devices.fs_sync(handle), 0);
        assert_eq!(devices.fs_remove("/home/hello.c"), STATUS_DENIED);
        assert_eq!(devices.fs_close(handle), 0);
        assert_eq!(devices.fs_close(handle), STATUS_BAD_HANDLE);

        let modified = devices.take_modified_files();
        assert_eq!(
            modified,
            vec![("/home/hello.c".to_owned(), b"hello".to_vec())]
        );
        assert!(devices.take_modified_files().is_empty());
        assert_eq!(devices.fs_remove("/home/hello.c"), 0);
        assert_eq!(devices.fs_stat("/home/hello.c"), None);
        assert_eq!(
            devices.take_removed_files(),
            vec!["/home/hello.c".to_owned()]
        );
        assert_eq!(devices.fs_remove("/home/hello.c"), STATUS_NOT_FOUND);
    }

    #[test]
    fn file_paths_are_confined_to_home() {
        let mut devices = ComputerDevices::new();
        for path in ["/etc/passwd", "/home/", "/home/../etc", "home/x", ""] {
            assert_eq!(
                devices.fs_open(path, FS_OPEN_READ | FS_OPEN_WRITE | FS_OPEN_CREATE),
                STATUS_INVALID,
                "path {path:?} must be rejected"
            );
        }
    }

    #[test]
    fn mounted_files_are_readable_without_modification_tracking() {
        let mut devices = ComputerDevices::new();
        devices
            .mount_file("/home/hello.c", b"seed".to_vec())
            .unwrap();
        assert!(devices.take_modified_files().is_empty());
        let handle = devices.fs_open("/home/hello.c", FS_OPEN_READ) as u32;
        let mut buffer = [0u8; 8];
        assert_eq!(devices.fs_read(handle, &mut buffer), 4);
        assert_eq!(&buffer[..4], b"seed");
        assert_eq!(devices.fs_write(handle, b"x"), STATUS_DENIED);
    }

    #[test]
    fn listing_record_is_length_delimited_and_sorted() {
        let mut devices = ComputerDevices::new();
        devices.mount_file("/home/b.txt", b"b".to_vec()).unwrap();
        devices.mount_file("/home/a.txt", b"a".to_vec()).unwrap();
        assert_eq!(
            devices.fs_list_record(),
            b"\x02\0\0\0\x0b\0\0\0/home/a.txt\x0b\0\0\0/home/b.txt"
        );
    }
}
