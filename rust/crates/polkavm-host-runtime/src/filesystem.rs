/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::computer::*;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

/// Complete filesystem metadata snapshot. Serialize alongside both byte delta drains.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemMetadata {
    pub version: u32,
    pub next_inode: String,
    pub clock_ns: String,
    pub entries: Vec<FilesystemMetadataEntry>,
}

/// Persistent identity and modification time of one canonical namespace entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemMetadataEntry {
    pub path: String,
    pub kind: u32,
    pub mtime_ns: String,
    pub inode: String,
}

struct Entry {
    bytes: Option<Vec<u8>>,
    mtime: u64,
    inode: u64,
}
impl Entry {
    fn kind(&self) -> u32 {
        if self.bytes.is_some() {
            1
        } else {
            2
        }
    }
    fn record(&self) -> [u8; 24] {
        let mut record = [0; 24];
        record[..4].copy_from_slice(&self.kind().to_le_bytes());
        record[4..8]
            .copy_from_slice(&(self.bytes.as_ref().map_or(0, Vec::len) as u32).to_le_bytes());
        record[8..16].copy_from_slice(&self.mtime.to_le_bytes());
        record[16..24].copy_from_slice(&self.inode.to_le_bytes());
        record
    }
}

pub(crate) struct Store {
    entries: BTreeMap<String, Entry>,
    opens: BTreeMap<String, usize>,
    modified: BTreeSet<String>,
    removed: BTreeSet<String>,
    next_inode: u64,
    clock: u64,
    dirty: bool,
}

pub(crate) type SharedFilesystem = Arc<Mutex<Store>>;
pub(crate) fn lock(store: &SharedFilesystem) -> MutexGuard<'_, Store> {
    store.lock().expect("filesystem mutex poisoned")
}

fn valid(path: &str) -> bool {
    path.len() <= MAX_COMPUTER_PATH_BYTES
        && !path.as_bytes().contains(&0)
        && (path == "/home" || path.starts_with("/home/"))
        && path[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').expect("canonical path").0
}
fn within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
fn decimal(value: &str) -> Result<u64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        bail!("invalid filesystem decimal integer");
    }
    Ok(value.parse()?)
}

impl Store {
    fn new() -> Self {
        Self {
            entries: BTreeMap::from([(
                "/home".to_owned(),
                Entry {
                    bytes: None,
                    mtime: 0,
                    inode: 1,
                },
            )]),
            opens: BTreeMap::new(),
            modified: BTreeSet::new(),
            removed: BTreeSet::new(),
            next_inode: 2,
            clock: 0,
            dirty: false,
        }
    }
    fn preflight(&self, creations: usize) -> Result<(), i32> {
        if self.clock == u64::MAX || self.next_inode.checked_add(creations as u64).is_none() {
            return Err(STATUS_LIMIT);
        }
        Ok(())
    }
    fn tick(&mut self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        #[cfg(target_arch = "wasm32")]
        let now = 0;
        self.clock = now.max(self.clock + 1);
        self.clock
    }
    fn insert(&mut self, path: String, bytes: Option<Vec<u8>>, time: u64) {
        let inode = self.next_inode;
        self.next_inode += 1;
        self.entries.insert(
            path,
            Entry {
                bytes,
                mtime: time,
                inode,
            },
        );
    }
    fn touch(&mut self, path: &str, time: u64) {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.mtime = time;
        }
        self.dirty = true;
    }
    fn changed(&mut self, path: &str, time: u64) {
        self.touch(path, time);
        if !self.modified.contains(path) {
            self.modified.insert(path.to_owned());
        }
        self.removed.remove(path);
    }
    fn deleted(&mut self, path: &str) {
        let entry = self.entries.remove(path).expect("preflighted entry");
        if entry.bytes.is_some() {
            self.modified.remove(path);
            self.removed.insert(path.to_owned());
        }
        self.dirty = true;
    }
    fn directory(&self, path: &str) -> Result<(), i32> {
        if self.lookup(path)?.bytes.is_some() {
            Err(STATUS_NOT_DIRECTORY)
        } else {
            Ok(())
        }
    }
    fn lookup(&self, path: &str) -> Result<&Entry, i32> {
        if !valid(path) {
            return Err(STATUS_INVALID);
        }
        if let Some(entry) = self.entries.get(path) {
            return Ok(entry);
        }
        let mut ancestor = parent(path);
        while ancestor.starts_with("/home") {
            if self
                .entries
                .get(ancestor)
                .is_some_and(|e| e.bytes.is_some())
            {
                return Err(STATUS_NOT_DIRECTORY);
            }
            ancestor = parent(ancestor);
        }
        Err(STATUS_NOT_FOUND)
    }
    fn count(&self, kind: u32) -> usize {
        self.entries.values().filter(|e| e.kind() == kind).count()
    }
    pub(crate) fn mount_file(&mut self, path: &str, bytes: Vec<u8>) -> Result<()> {
        if !valid(path) || path == "/home" {
            bail!("invalid computer file path {path:?}");
        }
        if bytes.len() > MAX_COMPUTER_FILE_BYTES {
            bail!("mounted file exceeds file size limit");
        }
        if self.opens.contains_key(path) {
            bail!("cannot replace an open file");
        }
        if self.entries.get(path).is_some_and(|e| e.bytes.is_none()) {
            bail!("mount destination is a directory");
        }
        let new_file = !self.entries.contains_key(path);
        if new_file && self.count(1) >= MAX_COMPUTER_FILES {
            bail!("computer filesystem file limit exceeded");
        }
        let mut missing = Vec::new();
        let mut ancestor = parent(path);
        while ancestor != "/home" {
            match self.entries.get(ancestor) {
                Some(entry) if entry.bytes.is_some() => bail!("mount ancestor is not a directory"),
                Some(_) => {}
                None => missing.push(ancestor.to_owned()),
            }
            ancestor = parent(ancestor);
        }
        if self.count(2) - 1 + missing.len() > MAX_COMPUTER_DIRECTORIES {
            bail!("computer directory limit exceeded");
        }
        if new_file {
            self.preflight(missing.len() + 1)
                .map_err(|s| anyhow::anyhow!("filesystem limit {s}"))?;
            let time = self.tick();
            for directory in missing.into_iter().rev() {
                self.insert(directory, None, time);
            }
            self.insert(path.to_owned(), Some(bytes), time);
        } else {
            self.entries.get_mut(path).unwrap().bytes = Some(bytes);
        }
        self.removed.remove(path);
        Ok(())
    }
    pub(crate) fn take_modified_files(&mut self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut self.modified)
            .into_iter()
            .filter_map(|path| {
                let bytes = self.entries.get(&path)?.bytes.as_ref()?.clone();
                Some((path, bytes))
            })
            .collect()
    }
    pub(crate) fn take_removed_files(&mut self) -> Vec<String> {
        std::mem::take(&mut self.removed).into_iter().collect()
    }
    pub(crate) fn export_metadata(&self) -> FilesystemMetadata {
        FilesystemMetadata {
            version: 1,
            next_inode: self.next_inode.to_string(),
            clock_ns: self.clock.to_string(),
            entries: self
                .entries
                .iter()
                .map(|(path, entry)| FilesystemMetadataEntry {
                    path: path.clone(),
                    kind: entry.kind(),
                    mtime_ns: entry.mtime.to_string(),
                    inode: entry.inode.to_string(),
                })
                .collect(),
        }
    }
    pub(crate) fn take_metadata(&mut self) -> Option<FilesystemMetadata> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        Some(self.export_metadata())
    }
    pub(crate) fn import_metadata(&mut self, metadata: FilesystemMetadata) -> Result<()> {
        if !self.opens.is_empty() {
            bail!("cannot restore metadata with open handles");
        }
        if metadata.version != 1 {
            bail!("unsupported filesystem metadata version");
        }
        let next = decimal(&metadata.next_inode)?;
        let clock = decimal(&metadata.clock_ns)?;
        let mut entries = BTreeMap::new();
        let mut inodes = BTreeSet::new();
        let mut files = BTreeSet::new();
        for entry in metadata.entries {
            let inode = decimal(&entry.inode)?;
            let mtime = decimal(&entry.mtime_ns)?;
            if !valid(&entry.path)
                || !matches!(entry.kind, 1 | 2)
                || inode == 0
                || inode >= next
                || mtime > clock
                || !inodes.insert(inode)
                || entries.contains_key(&entry.path)
            {
                bail!("invalid filesystem metadata entry");
            }
            if entry.kind == 1 {
                if !self
                    .entries
                    .get(&entry.path)
                    .is_some_and(|e| e.bytes.is_some())
                {
                    bail!("metadata file set differs from mounted bytes");
                }
                files.insert(entry.path.clone());
            }
            entries.insert(
                entry.path,
                Entry {
                    bytes: if entry.kind == 1 {
                        Some(Vec::new())
                    } else {
                        None
                    },
                    mtime,
                    inode,
                },
            );
        }
        if !entries.get("/home").is_some_and(|e| e.bytes.is_none())
            || entries.values().filter(|e| e.bytes.is_none()).count() - 1 > MAX_COMPUTER_DIRECTORIES
        {
            bail!("invalid filesystem root or directory quota");
        }
        if files.len() != self.count(1) {
            bail!("metadata file set differs from mounted bytes");
        }
        for path in entries.keys().filter(|p| p.as_str() != "/home") {
            if !entries.get(parent(path)).is_some_and(|e| e.bytes.is_none()) {
                bail!("metadata parent is missing or not a directory");
            }
        }
        // Every check precedes transferring bytes, so rejection leaves the store unchanged.
        for path in files {
            entries.get_mut(&path).unwrap().bytes =
                self.entries.get_mut(&path).unwrap().bytes.take();
        }
        self.entries = entries;
        self.next_inode = next;
        self.clock = clock;
        self.dirty = false;
        Ok(())
    }
}

struct OpenFile {
    path: String,
    position: usize,
    readable: bool,
    writable: bool,
    append: bool,
}

pub(crate) struct FileSession {
    pub(crate) shared: SharedFilesystem,
    handles: BTreeMap<u32, OpenFile>,
}
impl Drop for FileSession {
    fn drop(&mut self) {
        self.close_all();
    }
}
impl FileSession {
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Store::new())),
            handles: BTreeMap::new(),
        }
    }
    pub(crate) fn share(&mut self, shared: SharedFilesystem) {
        self.close_all();
        self.shared = shared;
    }
    pub(crate) fn close_all(&mut self) {
        let mut store = lock(&self.shared);
        for (_, open) in std::mem::take(&mut self.handles) {
            let count = store.opens.get_mut(&open.path).expect("tracked open path");
            *count -= 1;
            if *count == 0 {
                store.opens.remove(&open.path);
            }
        }
    }
    pub(crate) fn open(&mut self, path: &str, flags: u32) -> i32 {
        let writable = flags & FS_OPEN_WRITE != 0;
        let readable = flags & FS_OPEN_READ != 0;
        if !valid(path)
            || flags
                & !(FS_OPEN_READ
                    | FS_OPEN_WRITE
                    | FS_OPEN_CREATE
                    | FS_OPEN_TRUNCATE
                    | FS_OPEN_EXCLUSIVE
                    | FS_OPEN_APPEND)
                != 0
            || !(readable || writable)
            || (!writable
                && flags & (FS_OPEN_CREATE | FS_OPEN_TRUNCATE | FS_OPEN_EXCLUSIVE | FS_OPEN_APPEND)
                    != 0)
            || (flags & FS_OPEN_EXCLUSIVE != 0 && flags & FS_OPEN_CREATE == 0)
        {
            return STATUS_INVALID;
        }
        let mut store = lock(&self.shared);
        if flags & FS_OPEN_EXCLUSIVE != 0 && store.entries.contains_key(path) {
            return STATUS_EXISTS;
        }
        let exists = match store.lookup(path) {
            Ok(entry) if entry.bytes.is_none() => return STATUS_IS_DIRECTORY,
            Ok(_) => true,
            Err(STATUS_NOT_FOUND) => false,
            Err(status) => return status,
        };
        if self.handles.len() >= MAX_OPEN_COMPUTER_FILES {
            return STATUS_LIMIT;
        }
        if !exists {
            if flags & FS_OPEN_CREATE == 0 {
                return STATUS_NOT_FOUND;
            }
            if let Err(status) = store.directory(parent(path)) {
                return status;
            }
            if store.count(1) >= MAX_COMPUTER_FILES {
                return STATUS_LIMIT;
            }
        }
        if !exists || flags & FS_OPEN_TRUNCATE != 0 {
            if let Err(status) = store.preflight(usize::from(!exists)) {
                return status;
            }
            let time = store.tick();
            if exists {
                store
                    .entries
                    .get_mut(path)
                    .unwrap()
                    .bytes
                    .as_mut()
                    .unwrap()
                    .clear();
            } else {
                store.insert(path.to_owned(), Some(Vec::new()), time);
                store.touch(parent(path), time);
            }
            store.changed(path, time);
        }
        let handle = (FIRST_FILE_HANDLE..)
            .find(|h| !self.handles.contains_key(h))
            .unwrap();
        if let Some(count) = store.opens.get_mut(path) {
            *count += 1;
        } else {
            store.opens.insert(path.to_owned(), 1);
        }
        self.handles.insert(
            handle,
            OpenFile {
                path: path.to_owned(),
                position: 0,
                readable,
                writable,
                append: flags & FS_OPEN_APPEND != 0,
            },
        );
        handle as i32
    }
    pub(crate) fn read(&mut self, handle: u32, buffer: &mut [u8]) -> i32 {
        let Some(open) = self.handles.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        if !open.readable {
            return STATUS_DENIED;
        }
        let store = lock(&self.shared);
        let bytes = store.entries[&open.path].bytes.as_ref().unwrap();
        let start = open.position.min(bytes.len());
        let count = buffer.len().min(bytes.len() - start);
        buffer[..count].copy_from_slice(&bytes[start..start + count]);
        open.position = start + count;
        count as i32
    }
    pub(crate) fn write(&mut self, handle: u32, bytes: &[u8]) -> i32 {
        let Some(open) = self.handles.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        if !open.writable {
            return STATUS_DENIED;
        }
        if bytes.is_empty() {
            return 0;
        }
        let mut store = lock(&self.shared);
        let start = if open.append {
            store.entries[&open.path].bytes.as_ref().unwrap().len()
        } else {
            open.position
        };
        let end = start.saturating_add(bytes.len());
        if end > MAX_COMPUTER_FILE_BYTES {
            return STATUS_LIMIT;
        }
        if let Err(status) = store.preflight(0) {
            return status;
        }
        let time = store.tick();
        let file = store
            .entries
            .get_mut(&open.path)
            .unwrap()
            .bytes
            .as_mut()
            .unwrap();
        if file.len() < end {
            file.resize(end, 0);
        }
        file[start..end].copy_from_slice(bytes);
        open.position = end;
        store.changed(&open.path, time);
        bytes.len() as i32
    }
    pub(crate) fn seek(&mut self, handle: u32, offset: i32, whence: u32) -> i32 {
        let Some(open) = self.handles.get_mut(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        let store = lock(&self.shared);
        let base = match whence {
            0 => 0,
            1 => open.position as i64,
            2 => store.entries[&open.path].bytes.as_ref().unwrap().len() as i64,
            _ => return STATUS_INVALID,
        };
        let position = base + i64::from(offset);
        if !(0..=MAX_COMPUTER_FILE_BYTES as i64).contains(&position) {
            return STATUS_INVALID;
        }
        open.position = position as usize;
        position as i32
    }
    pub(crate) fn truncate(&mut self, handle: u32, length: u32) -> i32 {
        if length as usize > MAX_COMPUTER_FILE_BYTES {
            return STATUS_LIMIT;
        }
        let Some(open) = self.handles.get(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        if !open.writable {
            return STATUS_DENIED;
        }
        let mut store = lock(&self.shared);
        if let Err(status) = store.preflight(0) {
            return status;
        }
        let time = store.tick();
        store
            .entries
            .get_mut(&open.path)
            .unwrap()
            .bytes
            .as_mut()
            .unwrap()
            .resize(length as usize, 0);
        store.changed(&open.path, time);
        0
    }
    pub(crate) fn stat(&self, path: &str) -> Option<u32> {
        lock(&self.shared)
            .lookup(path)
            .ok()?
            .bytes
            .as_ref()
            .map(|b| b.len() as u32)
    }
    pub(crate) fn metadata(&self, path: &str) -> Result<[u8; 24], i32> {
        Ok(lock(&self.shared).lookup(path)?.record())
    }
    pub(crate) fn fstat(&self, handle: u32) -> Result<[u8; 24], i32> {
        let open = self.handles.get(&handle).ok_or(STATUS_BAD_HANDLE)?;
        self.metadata(&open.path)
    }
    /// Visibility only: writes are already shared; this does not promise durable persistence.
    pub(crate) fn sync(&self, handle: u32) -> i32 {
        if self.handles.contains_key(&handle) {
            0
        } else {
            STATUS_BAD_HANDLE
        }
    }
    pub(crate) fn close(&mut self, handle: u32) -> i32 {
        let Some(open) = self.handles.remove(&handle) else {
            return STATUS_BAD_HANDLE;
        };
        let mut store = lock(&self.shared);
        let count = store.opens.get_mut(&open.path).unwrap();
        *count -= 1;
        if *count == 0 {
            store.opens.remove(&open.path);
        }
        0
    }
    pub(crate) fn remove(&mut self, path: &str) -> i32 {
        let mut store = lock(&self.shared);
        match store.lookup(path) {
            Ok(entry) if entry.bytes.is_none() => return STATUS_IS_DIRECTORY,
            Err(status) => return status,
            _ => {}
        }
        if store.opens.contains_key(path) {
            return STATUS_DENIED;
        }
        if let Err(status) = store.preflight(0) {
            return status;
        }
        let time = store.tick();
        store.deleted(path);
        store.touch(parent(path), time);
        0
    }
    pub(crate) fn mkdir(&mut self, path: &str) -> i32 {
        if !valid(path) {
            return STATUS_INVALID;
        }
        let mut store = lock(&self.shared);
        if store.entries.contains_key(path) {
            return STATUS_EXISTS;
        }
        if let Err(status) = store.directory(parent(path)) {
            return status;
        }
        if store.count(2) > MAX_COMPUTER_DIRECTORIES {
            return STATUS_LIMIT;
        }
        if let Err(status) = store.preflight(1) {
            return status;
        }
        let time = store.tick();
        store.insert(path.to_owned(), None, time);
        store.touch(parent(path), time);
        0
    }
    pub(crate) fn rmdir(&mut self, path: &str) -> i32 {
        if !valid(path) {
            return STATUS_INVALID;
        }
        if path == "/home" {
            return STATUS_DENIED;
        }
        let mut store = lock(&self.shared);
        if let Err(status) = store.directory(path) {
            return status;
        }
        if store.opens.keys().any(|p| within(p, path)) {
            return STATUS_DENIED;
        }
        if store.entries.keys().any(|p| p != path && within(p, path)) {
            return STATUS_NOT_EMPTY;
        }
        if let Err(status) = store.preflight(0) {
            return status;
        }
        let time = store.tick();
        store.deleted(path);
        store.touch(parent(path), time);
        0
    }
    pub(crate) fn rename(&mut self, old: &str, new: &str) -> i32 {
        if !valid(old) || !valid(new) {
            return STATUS_INVALID;
        }
        let mut store = lock(&self.shared);
        let kind = match store.lookup(old) {
            Ok(entry) => entry.kind(),
            Err(status) => return status,
        };
        if old == new {
            return 0;
        }
        if old == "/home" || new == "/home" {
            return STATUS_DENIED;
        }
        if kind == 2 && within(new, old) {
            return STATUS_INVALID;
        }
        if let Err(status) = store.directory(parent(new)) {
            return status;
        }
        if let Some(dest) = store.entries.get(new) {
            if kind != dest.kind() {
                return if kind == 1 {
                    STATUS_IS_DIRECTORY
                } else {
                    STATUS_NOT_DIRECTORY
                };
            }
        }
        if store.opens.keys().any(|p| within(p, old) || within(p, new)) {
            return STATUS_DENIED;
        }
        if kind == 2 && store.entries.keys().any(|p| p != new && within(p, new)) {
            return STATUS_NOT_EMPTY;
        }
        let moves: Vec<_> = store
            .entries
            .keys()
            .filter(|p| within(p, old))
            .map(|p| (p.clone(), format!("{new}{}", &p[old.len()..])))
            .collect();
        if moves.iter().any(|(_, target)| !valid(target)) {
            return STATUS_INVALID;
        }
        if let Err(status) = store.preflight(0) {
            return status;
        }
        let time = store.tick();
        if store.entries.contains_key(new) {
            store.deleted(new);
        }
        for (source, target) in moves {
            let entry = store.entries.remove(&source).unwrap();
            if entry.bytes.is_some() {
                store.modified.remove(&source);
                store.removed.insert(source);
                store.modified.insert(target.clone());
                store.removed.remove(&target);
            }
            store.entries.insert(target, entry);
        }
        store.touch(parent(old), time);
        store.touch(parent(new), time);
        0
    }
    pub(crate) fn list(&self) -> Vec<u8> {
        let store = lock(&self.shared);
        let mut record = Vec::new();
        record.extend_from_slice(&(store.count(1) as u32).to_le_bytes());
        for (path, _) in store.entries.iter().filter(|(_, e)| e.bytes.is_some()) {
            record.extend_from_slice(&(path.len() as u32).to_le_bytes());
            record.extend_from_slice(path.as_bytes());
        }
        record
    }
    pub(crate) fn list_directory(&self, path: &str) -> Result<Vec<u8>, i32> {
        if !valid(path) {
            return Err(STATUS_INVALID);
        }
        let store = lock(&self.shared);
        store.directory(path)?;
        let mut record = vec![0; 4];
        let mut count = 0u32;
        for (child, entry) in &store.entries {
            if child == "/home" || parent(child) != path {
                continue;
            }
            let name = &child[path.len() + 1..];
            record.extend_from_slice(&(name.len() as u32).to_le_bytes());
            record.extend_from_slice(name.as_bytes());
            record.extend_from_slice(&entry.kind().to_le_bytes());
            count += 1;
        }
        record[..4].copy_from_slice(&count.to_le_bytes());
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(session: &FileSession) -> FileSession {
        let mut peer = FileSession::new();
        peer.share(session.shared.clone());
        peer
    }

    fn bytes(session: &mut FileSession, path: &str) -> Vec<u8> {
        let handle = session.open(path, FS_OPEN_READ);
        assert!(handle >= FIRST_FILE_HANDLE as i32);
        let mut output = vec![0; session.stat(path).unwrap() as usize];
        assert_eq!(
            session.read(handle as u32, &mut output),
            output.len() as i32
        );
        assert_eq!(session.close(handle as u32), 0);
        output
    }

    #[test]
    fn exclusive_creation_and_open_locks_are_shared_and_released_on_drop() {
        let mut root = FileSession::new();
        let mut child = peer(&root);
        let flags = FS_OPEN_WRITE | FS_OPEN_CREATE | FS_OPEN_EXCLUSIVE;
        let handle = child.open("/home/index.lock", flags) as u32;
        assert_eq!(child.write(handle, b"index"), 5);
        assert_eq!(
            root.open("/home/index.lock", flags | FS_OPEN_TRUNCATE),
            STATUS_EXISTS
        );
        assert_eq!(root.remove("/home/index.lock"), STATUS_DENIED);
        assert_eq!(
            root.rename("/home/index.lock", "/home/index"),
            STATUS_DENIED
        );
        assert!(lock(&root.shared)
            .mount_file("/home/index.lock", b"bad".to_vec())
            .is_err());
        drop(child);
        assert_eq!(root.rename("/home/index.lock", "/home/index"), 0);
        assert_eq!(bytes(&mut root, "/home/index"), b"index");
        assert_eq!(
            root.open("/home/index.lock", flags),
            FIRST_FILE_HANDLE as i32
        );
    }

    #[test]
    fn append_uses_shared_eof_instead_of_each_handles_seek_position() {
        let mut first = FileSession::new();
        let mut second = peer(&first);
        let flags = FS_OPEN_WRITE | FS_OPEN_CREATE | FS_OPEN_APPEND;
        let a = first.open("/home/log", flags) as u32;
        let b = second.open("/home/log", flags) as u32;
        assert_eq!(first.write(a, b"one"), 3);
        assert_eq!(second.write(b, b"two"), 3);
        assert_eq!(first.seek(a, 0, 0), 0);
        assert_eq!(first.write(a, b"three"), 5);
        assert_eq!(bytes(&mut second, "/home/log"), b"onetwothree");
        assert_eq!(
            lock(&first.shared).take_modified_files(),
            vec![("/home/log".into(), b"onetwothree".to_vec())]
        );
    }

    #[test]
    fn rename_moves_subtree_and_preserves_file_identity_and_modification_time() {
        let mut fs = FileSession::new();
        lock(&fs.shared)
            .mount_file("/home/old/sub/file", b"data".to_vec())
            .unwrap();
        assert_eq!(fs.mkdir("/home/empty"), 0);
        let identity = fs.metadata("/home/old/sub/file").unwrap();
        assert_eq!(fs.rename("/home/old", "/home/empty"), 0);
        assert_eq!(fs.metadata("/home/empty/sub/file").unwrap(), identity);
        assert_eq!(fs.metadata("/home/old"), Err(STATUS_NOT_FOUND));
        assert_eq!(bytes(&mut fs, "/home/empty/sub/file"), b"data");
        assert_eq!(
            lock(&fs.shared).take_removed_files(),
            vec!["/home/old/sub/file"]
        );
        assert_eq!(
            lock(&fs.shared).take_modified_files(),
            vec![("/home/empty/sub/file".into(), b"data".to_vec())]
        );
    }

    #[test]
    fn rename_failures_preserve_destination_namespace_bytes_and_metadata() {
        let mut fs = FileSession::new();
        lock(&fs.shared)
            .mount_file("/home/source/sub/file", b"source".to_vec())
            .unwrap();
        lock(&fs.shared)
            .mount_file("/home/dest/file", b"destination".to_vec())
            .unwrap();
        let before = lock(&fs.shared).export_metadata();
        assert_eq!(fs.rename("/home/source", "/home/dest"), STATUS_NOT_EMPTY);
        assert_eq!(
            fs.rename("/home/source", "/home/source/sub/loop"),
            STATUS_INVALID
        );
        let too_long = format!("/home/{}", "x".repeat(MAX_COMPUTER_PATH_BYTES - 6));
        assert_eq!(fs.rename("/home/source", &too_long), STATUS_INVALID);
        let mut child = peer(&fs);
        let handle = child.open("/home/dest/file", FS_OPEN_READ) as u32;
        assert_eq!(
            fs.rename("/home/source/sub/file", "/home/dest/file"),
            STATUS_DENIED
        );
        assert_eq!(lock(&fs.shared).export_metadata(), before);
        assert!(lock(&fs.shared).take_metadata().is_none());
        assert!(lock(&fs.shared).take_modified_files().is_empty());
        assert!(lock(&fs.shared).take_removed_files().is_empty());
        assert_eq!(bytes(&mut fs, "/home/dest/file"), b"destination");
        assert_eq!(child.close(handle), 0);
        assert_eq!(fs.rename("/home/source/sub/file", "/home/dest/file"), 0);
        assert_eq!(bytes(&mut fs, "/home/dest/file"), b"source");
    }

    #[test]
    fn directory_discovery_is_immediate_typed_and_sorted_by_utf8() {
        let mut fs = FileSession::new();
        lock(&fs.shared)
            .mount_file("/home/z/hidden", vec![])
            .unwrap();
        lock(&fs.shared).mount_file("/home/é", vec![]).unwrap();
        lock(&fs.shared).mount_file("/home/a", vec![]).unwrap();
        assert_eq!(
            fs.list_directory("/home").unwrap(),
            b"\x03\0\0\0\x01\0\0\0a\x01\0\0\0\x01\0\0\0z\x02\0\0\0\x02\0\0\0\xc3\xa9\x01\0\0\0"
        );
        assert_eq!(fs.list_directory("/home/a"), Err(STATUS_NOT_DIRECTORY));
        assert_eq!(fs.remove("/home/z"), STATUS_IS_DIRECTORY);
        assert_eq!(fs.rmdir("/home/z"), STATUS_NOT_EMPTY);
        assert_eq!(
            fs.open("/home/missing/file", FS_OPEN_WRITE | FS_OPEN_CREATE),
            STATUS_NOT_FOUND
        );
        assert_eq!(fs.mkdir("/home/a/child"), STATUS_NOT_DIRECTORY);
        assert_eq!(fs.open("/home/z", FS_OPEN_READ), STATUS_IS_DIRECTORY);
    }

    #[test]
    fn quota_failures_do_not_create_partial_mount_parents_or_truncate_files() {
        let mut fs = FileSession::new();
        for index in 0..MAX_COMPUTER_DIRECTORIES {
            assert_eq!(fs.mkdir(&format!("/home/d{index}")), 0);
        }
        lock(&fs.shared).take_metadata();
        let before = lock(&fs.shared).export_metadata();
        assert!(lock(&fs.shared)
            .mount_file("/home/new/parents/file", vec![])
            .is_err());
        assert_eq!(fs.mkdir("/home/overflow"), STATUS_LIMIT);
        assert_eq!(lock(&fs.shared).export_metadata(), before);
        assert!(lock(&fs.shared).take_metadata().is_none());
        for index in 0..MAX_COMPUTER_FILES {
            lock(&fs.shared)
                .mount_file(&format!("/home/f{index}"), b"keep".to_vec())
                .unwrap();
        }
        assert_eq!(
            fs.open("/home/extra", FS_OPEN_WRITE | FS_OPEN_CREATE),
            STATUS_LIMIT
        );
        for _ in 0..MAX_OPEN_COMPUTER_FILES {
            assert!(fs.open("/home/f0", FS_OPEN_READ) > 0);
        }
        let before = fs.metadata("/home/f0").unwrap();
        assert_eq!(
            fs.open("/home/f0", FS_OPEN_WRITE | FS_OPEN_TRUNCATE),
            STATUS_LIMIT
        );
        assert_eq!(fs.metadata("/home/f0").unwrap(), before);
        fs.close_all();
        assert_eq!(bytes(&mut fs, "/home/f0"), b"keep");
    }

    #[test]
    fn strict_flags_and_noncanonical_paths_never_mutate() {
        let mut fs = FileSession::new();
        lock(&fs.shared)
            .mount_file("/home/file", b"keep".to_vec())
            .unwrap();
        let before = lock(&fs.shared).export_metadata();
        for flags in [
            FS_OPEN_READ | FS_OPEN_TRUNCATE,
            FS_OPEN_WRITE | FS_OPEN_EXCLUSIVE,
            FS_OPEN_READ | FS_OPEN_APPEND,
            FS_OPEN_WRITE | FS_OPEN_CREATE | 64,
        ] {
            assert_eq!(fs.open("/home/file", flags), STATUS_INVALID);
        }
        for path in [
            "/home//x",
            "/home/x/",
            "/home/./x",
            "/home/x/../y",
            "/home/x\0",
        ] {
            assert_eq!(
                fs.open(path, FS_OPEN_WRITE | FS_OPEN_CREATE),
                STATUS_INVALID
            );
        }
        assert_eq!(lock(&fs.shared).export_metadata(), before);
        assert_eq!(bytes(&mut fs, "/home/file"), b"keep");
    }

    #[test]
    fn same_size_writes_advance_restored_clock_and_invalid_restore_is_atomic() {
        let mut fs = FileSession::new();
        lock(&fs.shared)
            .mount_file("/home/file", b"old".to_vec())
            .unwrap();
        assert_eq!(fs.mkdir("/home/empty"), 0);
        let mut initial = lock(&fs.shared).export_metadata();
        // A future persisted clock makes this a deterministic wall-clock rollback.
        initial.clock_ns = "18000000000000000000".into();
        for entry in &mut initial.entries {
            entry.mtime_ns = initial.clock_ns.clone();
        }
        lock(&fs.shared).import_metadata(initial.clone()).unwrap();
        assert!(lock(&fs.shared).take_metadata().is_none());
        let before = fs.metadata("/home/file").unwrap();
        let handle = fs.open("/home/file", FS_OPEN_WRITE) as u32;
        assert!(lock(&fs.shared).import_metadata(initial.clone()).is_err());
        assert_eq!(fs.write(handle, b"new"), 3);
        let after = fs.fstat(handle).unwrap();
        assert_eq!(&after[..8], &before[..8]);
        assert_eq!(&after[16..], &before[16..]);
        assert_eq!(
            u64::from_le_bytes(after[8..16].try_into().unwrap()),
            18_000_000_000_000_000_001
        );
        assert_eq!(fs.close(handle), 0);
        let snapshot = lock(&fs.shared).take_metadata().unwrap();
        assert!(lock(&fs.shared).take_metadata().is_none());
        let mut restored = FileSession::new();
        lock(&restored.shared)
            .mount_file("/home/file", b"new".to_vec())
            .unwrap();
        lock(&restored.shared)
            .import_metadata(snapshot.clone())
            .unwrap();
        assert_eq!(restored.metadata("/home/file").unwrap(), after);
        assert_eq!(restored.list_directory("/home/empty").unwrap(), vec![0; 4]);
        let mut invalid = snapshot.clone();
        invalid.entries.retain(|entry| entry.path != "/home/file");
        assert!(lock(&restored.shared).import_metadata(invalid).is_err());
        assert_eq!(lock(&restored.shared).export_metadata(), snapshot);
        assert_eq!(bytes(&mut restored, "/home/file"), b"new");
    }

    #[test]
    fn exhausted_counters_restore_but_reject_mutation_without_partial_changes() {
        let mut fs = FileSession::new();
        lock(&fs.shared)
            .mount_file("/home/file", b"keep".to_vec())
            .unwrap();
        let mut metadata = lock(&fs.shared).export_metadata();
        metadata.clock_ns = u64::MAX.to_string();
        metadata.next_inode = u64::MAX.to_string();
        lock(&fs.shared).import_metadata(metadata.clone()).unwrap();
        let handle = fs.open("/home/file", FS_OPEN_WRITE) as u32;
        assert_eq!(fs.write(handle, b"lost"), STATUS_LIMIT);
        assert_eq!(fs.close(handle), 0);
        assert_eq!(
            fs.open("/home/new", FS_OPEN_WRITE | FS_OPEN_CREATE),
            STATUS_LIMIT
        );
        assert_eq!(fs.rename("/home/file", "/home/new"), STATUS_LIMIT);
        assert_eq!(lock(&fs.shared).export_metadata(), metadata);
        assert_eq!(bytes(&mut fs, "/home/file"), b"keep");
        assert!(lock(&fs.shared).take_metadata().is_none());
        assert!(lock(&fs.shared).take_modified_files().is_empty());
    }
}
