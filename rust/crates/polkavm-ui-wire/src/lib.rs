/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Bounded UI platform-output wire values shared by PolkaVM guests and Hosts.

#![no_std]

extern crate alloc;

use alloc::collections::TryReserveError;
use alloc::vec::Vec;
use core::fmt;

/// Import name used by PolkaVM guests to submit one UI platform-output snapshot.
pub const UI_OUTPUT_SUBMIT_IMPORT: &str = "host_ui_output_submit";
/// Four-byte discriminator at the start of every UI output v1 stream.
pub const UI_OUTPUT_MAGIC: [u8; 4] = *b"PUI1";
/// Current UI output wire version.
pub const UI_OUTPUT_VERSION: u16 = 1;
/// Encoded byte length of the UI output v1 header.
pub const UI_OUTPUT_HEADER_BYTES: usize = 48;
/// Encoded byte length of every UI output command header.
pub const UI_OUTPUT_COMMAND_HEADER_BYTES: usize = 8;
/// Maximum complete UI output stream accepted by the Host.
pub const MAX_UI_OUTPUT_BYTES: usize = 256 * 1024;
/// Maximum commands accepted in one UI output stream.
pub const MAX_UI_OUTPUT_COMMANDS: usize = 64;
/// Maximum UTF-8 clipboard text accepted in one command.
pub const MAX_UI_COPY_TEXT_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 URL accepted in one command.
pub const MAX_UI_OPEN_URL_BYTES: usize = 8 * 1024;

/// The UI output was accepted.
pub const UI_OUTPUT_SUBMIT_ACCEPTED: u32 = 0;
/// The UI output was malformed, out of bounds, or over a limit.
pub const UI_OUTPUT_SUBMIT_INVALID: u32 = 1;
/// A UI output was already submitted during the current guest call.
pub const UI_OUTPUT_SUBMIT_DUPLICATE: u32 = 2;

/// The pointer is currently over mutable text.
pub const UI_OUTPUT_FLAG_MUTABLE_TEXT: u8 = 1 << 0;
/// The header contains active IME geometry.
pub const UI_OUTPUT_FLAG_IME: u8 = 1 << 1;
const UI_OUTPUT_FLAGS_V1: u8 = UI_OUTPUT_FLAG_MUTABLE_TEXT | UI_OUTPUT_FLAG_IME;

/// Copy UTF-8 text to the platform clipboard.
pub const UI_OUTPUT_COMMAND_COPY_TEXT: u8 = 1;
/// Open a UTF-8 URL through the Host's navigation policy.
pub const UI_OUTPUT_COMMAND_OPEN_URL: u8 = 2;
/// Open the URL in a new platform surface rather than replacing the current one.
pub const UI_OUTPUT_OPEN_URL_NEW_SURFACE: u8 = 1 << 0;

/// A surface-relative rectangle in logical UI points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiRect {
    /// Left edge.
    pub min_x: f32,
    /// Top edge.
    pub min_y: f32,
    /// Right edge.
    pub max_x: f32,
    /// Bottom edge.
    pub max_y: f32,
}

impl UiRect {
    fn is_valid(self) -> bool {
        [self.min_x, self.min_y, self.max_x, self.max_y]
            .iter()
            .all(|value| value.is_finite())
            && self.max_x >= self.min_x
            && self.max_y >= self.min_y
    }
}

/// Geometry for an active text editor and its primary cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiImeOutput {
    /// Bounds of the text editor.
    pub rect: UiRect,
    /// Bounds of the primary text cursor.
    pub cursor_rect: UiRect,
}

impl UiImeOutput {
    fn is_valid(self) -> bool {
        self.rect.is_valid() && self.cursor_rect.is_valid()
    }
}

/// Host cursor requested by the application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum UiCursorIcon {
    /// Platform default cursor.
    #[default]
    Default = 0,
    /// Hide the cursor.
    None = 1,
    /// A context menu is available.
    ContextMenu = 2,
    /// Help is available.
    Help = 3,
    /// Pointing hand for a link or action.
    PointingHand = 4,
    /// Work is in progress while interaction remains possible.
    Progress = 5,
    /// Work is in progress and interaction should wait.
    Wait = 6,
    /// Table-cell selection.
    Cell = 7,
    /// Precision crosshair.
    Crosshair = 8,
    /// Horizontal text selection.
    Text = 9,
    /// Vertical text selection.
    VerticalText = 10,
    /// Alias or shortcut operation.
    Alias = 11,
    /// Copy operation.
    Copy = 12,
    /// Move operation.
    Move = 13,
    /// The current item cannot be dropped here.
    NoDrop = 14,
    /// The requested operation is forbidden.
    NotAllowed = 15,
    /// The item can be grabbed.
    Grab = 16,
    /// The item is being grabbed.
    Grabbing = 17,
    /// Bidirectional scrolling.
    AllScroll = 18,
    /// Horizontal resize.
    ResizeHorizontal = 19,
    /// North-east to south-west resize.
    ResizeNeSw = 20,
    /// North-west to south-east resize.
    ResizeNwSe = 21,
    /// Vertical resize.
    ResizeVertical = 22,
    /// East-edge resize.
    ResizeEast = 23,
    /// South-east-edge resize.
    ResizeSouthEast = 24,
    /// South-edge resize.
    ResizeSouth = 25,
    /// South-west-edge resize.
    ResizeSouthWest = 26,
    /// West-edge resize.
    ResizeWest = 27,
    /// North-west-edge resize.
    ResizeNorthWest = 28,
    /// North-edge resize.
    ResizeNorth = 29,
    /// North-east-edge resize.
    ResizeNorthEast = 30,
    /// Column resize.
    ResizeColumn = 31,
    /// Row resize.
    ResizeRow = 32,
    /// Zoom in.
    ZoomIn = 33,
    /// Zoom out.
    ZoomOut = 34,
}

impl TryFrom<u8> for UiCursorIcon {
    type Error = UiOutputDecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Default,
            1 => Self::None,
            2 => Self::ContextMenu,
            3 => Self::Help,
            4 => Self::PointingHand,
            5 => Self::Progress,
            6 => Self::Wait,
            7 => Self::Cell,
            8 => Self::Crosshair,
            9 => Self::Text,
            10 => Self::VerticalText,
            11 => Self::Alias,
            12 => Self::Copy,
            13 => Self::Move,
            14 => Self::NoDrop,
            15 => Self::NotAllowed,
            16 => Self::Grab,
            17 => Self::Grabbing,
            18 => Self::AllScroll,
            19 => Self::ResizeHorizontal,
            20 => Self::ResizeNeSw,
            21 => Self::ResizeNwSe,
            22 => Self::ResizeVertical,
            23 => Self::ResizeEast,
            24 => Self::ResizeSouthEast,
            25 => Self::ResizeSouth,
            26 => Self::ResizeSouthWest,
            27 => Self::ResizeWest,
            28 => Self::ResizeNorthWest,
            29 => Self::ResizeNorth,
            30 => Self::ResizeNorthEast,
            31 => Self::ResizeColumn,
            32 => Self::ResizeRow,
            33 => Self::ZoomIn,
            34 => Self::ZoomOut,
            _ => return Err(UiOutputDecodeError::CursorIcon),
        })
    }
}

/// Persistent UI integration state included in one output snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiOutputDescriptor {
    /// Cursor requested for the application surface.
    pub cursor_icon: UiCursorIcon,
    /// Whether the pointer is over mutable text.
    pub mutable_text_under_cursor: bool,
    /// Active text-editor geometry, when editing text.
    pub ime: Option<UiImeOutput>,
}

/// One decoded ephemeral UI command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiOutputCommand<'a> {
    /// Replace the platform clipboard text.
    CopyText(&'a str),
    /// Navigate to a URL through Host policy.
    OpenUrl {
        /// Untrusted URL requested by the guest.
        url: &'a str,
        /// Whether a new platform surface was requested.
        new_surface: bool,
    },
}

/// A validated borrowed UI output stream.
#[derive(Clone, Copy, Debug)]
pub struct UiOutput<'a> {
    bytes: &'a [u8],
    descriptor: UiOutputDescriptor,
    command_count: u16,
}

impl<'a> UiOutput<'a> {
    /// Persistent state requested by this snapshot.
    pub fn descriptor(self) -> UiOutputDescriptor {
        self.descriptor
    }

    /// Number of ephemeral commands in this snapshot.
    pub fn command_count(self) -> usize {
        usize::from(self.command_count)
    }

    /// Iterate over validated commands in submission order.
    pub fn commands(self) -> UiOutputCommands<'a> {
        UiOutputCommands {
            bytes: self.bytes,
            offset: UI_OUTPUT_HEADER_BYTES,
            remaining: self.command_count,
        }
    }
}

/// Iterator over commands in a validated UI output stream.
#[derive(Clone, Debug)]
pub struct UiOutputCommands<'a> {
    bytes: &'a [u8],
    offset: usize,
    remaining: u16,
}

impl<'a> Iterator for UiOutputCommands<'a> {
    type Item = UiOutputCommand<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let opcode = self.bytes[self.offset];
        let flags = self.bytes[self.offset + 1];
        let length = read_u32(self.bytes, self.offset + 4) as usize;
        let start = self.offset + UI_OUTPUT_COMMAND_HEADER_BYTES;
        let end = start + length;
        let payload =
            core::str::from_utf8(&self.bytes[start..end]).expect("validated UI command text");
        self.offset = end;
        self.remaining -= 1;
        Some(match opcode {
            UI_OUTPUT_COMMAND_COPY_TEXT => UiOutputCommand::CopyText(payload),
            UI_OUTPUT_COMMAND_OPEN_URL => UiOutputCommand::OpenUrl {
                url: payload,
                new_surface: flags & UI_OUTPUT_OPEN_URL_NEW_SURFACE != 0,
            },
            _ => unreachable!("validated UI command opcode"),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = usize::from(self.remaining);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for UiOutputCommands<'_> {}

/// Incremental encoder that reuses a caller-owned byte buffer.
pub struct UiOutputEncoder<'a> {
    destination: &'a mut Vec<u8>,
    command_count: u16,
}

impl<'a> UiOutputEncoder<'a> {
    /// Begin a stream, clearing and retaining the destination allocation.
    pub fn begin(
        destination: &'a mut Vec<u8>,
        descriptor: UiOutputDescriptor,
    ) -> Result<Self, UiOutputEncodeError> {
        if descriptor.ime.is_some_and(|ime| !ime.is_valid()) {
            return Err(UiOutputEncodeError::ImeBounds);
        }
        destination.clear();
        destination
            .try_reserve(UI_OUTPUT_HEADER_BYTES)
            .map_err(UiOutputEncodeError::Allocation)?;
        destination.extend_from_slice(&UI_OUTPUT_MAGIC);
        push_u16(destination, UI_OUTPUT_VERSION);
        push_u16(destination, UI_OUTPUT_HEADER_BYTES as u16);
        push_u32(destination, 0);
        push_u16(destination, 0);
        destination.push(descriptor.cursor_icon as u8);
        let mut flags = 0;
        if descriptor.mutable_text_under_cursor {
            flags |= UI_OUTPUT_FLAG_MUTABLE_TEXT;
        }
        if descriptor.ime.is_some() {
            flags |= UI_OUTPUT_FLAG_IME;
        }
        destination.push(flags);
        let ime = descriptor.ime.unwrap_or_default();
        for value in [
            ime.rect.min_x,
            ime.rect.min_y,
            ime.rect.max_x,
            ime.rect.max_y,
            ime.cursor_rect.min_x,
            ime.cursor_rect.min_y,
            ime.cursor_rect.max_x,
            ime.cursor_rect.max_y,
        ] {
            push_u32(destination, value.to_bits());
        }
        Ok(Self {
            destination,
            command_count: 0,
        })
    }

    /// Append one clipboard-text command.
    pub fn copy_text(&mut self, text: &str) -> Result<(), UiOutputEncodeError> {
        if text.len() > MAX_UI_COPY_TEXT_BYTES {
            return Err(UiOutputEncodeError::CopyTextLength);
        }
        self.push_command(UI_OUTPUT_COMMAND_COPY_TEXT, 0, text.as_bytes())
    }

    /// Append one URL navigation command.
    pub fn open_url(&mut self, url: &str, new_surface: bool) -> Result<(), UiOutputEncodeError> {
        if url.is_empty() || url.len() > MAX_UI_OPEN_URL_BYTES {
            return Err(UiOutputEncodeError::OpenUrlLength);
        }
        self.push_command(
            UI_OUTPUT_COMMAND_OPEN_URL,
            u8::from(new_surface) * UI_OUTPUT_OPEN_URL_NEW_SURFACE,
            url.as_bytes(),
        )
    }

    /// Finish the stream by writing its total length and command count.
    pub fn finish(self) {
        let length = self.destination.len() as u32;
        self.destination[8..12].copy_from_slice(&length.to_le_bytes());
        self.destination[12..14].copy_from_slice(&self.command_count.to_le_bytes());
    }

    fn push_command(
        &mut self,
        opcode: u8,
        flags: u8,
        payload: &[u8],
    ) -> Result<(), UiOutputEncodeError> {
        if usize::from(self.command_count) == MAX_UI_OUTPUT_COMMANDS {
            return Err(UiOutputEncodeError::CommandCount);
        }
        let added = UI_OUTPUT_COMMAND_HEADER_BYTES
            .checked_add(payload.len())
            .ok_or(UiOutputEncodeError::OutputLength)?;
        let next_length = self
            .destination
            .len()
            .checked_add(added)
            .ok_or(UiOutputEncodeError::OutputLength)?;
        if next_length > MAX_UI_OUTPUT_BYTES || next_length > u32::MAX as usize {
            return Err(UiOutputEncodeError::OutputLength);
        }
        self.destination
            .try_reserve(added)
            .map_err(UiOutputEncodeError::Allocation)?;
        self.destination.push(opcode);
        self.destination.push(flags);
        push_u16(self.destination, 0);
        push_u32(self.destination, payload.len() as u32);
        self.destination.extend_from_slice(payload);
        self.command_count += 1;
        Ok(())
    }
}

/// Encode failure before a stream is submitted to a Host.
#[derive(Debug)]
pub enum UiOutputEncodeError {
    /// Active IME geometry is non-finite or unordered.
    ImeBounds,
    /// The command count exceeds [`MAX_UI_OUTPUT_COMMANDS`].
    CommandCount,
    /// Clipboard text exceeds [`MAX_UI_COPY_TEXT_BYTES`].
    CopyTextLength,
    /// A URL is empty or exceeds [`MAX_UI_OPEN_URL_BYTES`].
    OpenUrlLength,
    /// The complete stream exceeds [`MAX_UI_OUTPUT_BYTES`].
    OutputLength,
    /// The destination could not reserve enough memory.
    Allocation(TryReserveError),
}

impl PartialEq for UiOutputEncodeError {
    fn eq(&self, other: &Self) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }
}

impl Eq for UiOutputEncodeError {}

impl fmt::Display for UiOutputEncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ImeBounds => "invalid UI IME bounds",
            Self::CommandCount => "too many UI output commands",
            Self::CopyTextLength => "UI clipboard text exceeds the wire limit",
            Self::OpenUrlLength => "UI URL is empty or exceeds the wire limit",
            Self::OutputLength => "UI output exceeds the wire limit",
            Self::Allocation(_) => "could not allocate the UI output stream",
        })
    }
}

impl core::error::Error for UiOutputEncodeError {}

/// UI output v1 decoding or validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiOutputDecodeError {
    /// The complete byte length is outside the wire bounds.
    Length,
    /// The stream discriminator is not [`UI_OUTPUT_MAGIC`].
    Magic,
    /// The stream version is unsupported.
    Version,
    /// The header size or encoded total size is invalid.
    Header,
    /// The stream contains unknown header flags.
    Flags,
    /// The cursor value is unsupported.
    CursorIcon,
    /// IME geometry is non-finite, unordered, or noncanonical.
    ImeBounds,
    /// The command count exceeds [`MAX_UI_OUTPUT_COMMANDS`].
    CommandCount,
    /// A command header or payload is truncated.
    CommandLength,
    /// A command opcode is unsupported.
    CommandOpcode,
    /// A command contains unsupported flags or reserved data.
    CommandFlags,
    /// Clipboard text exceeds [`MAX_UI_COPY_TEXT_BYTES`].
    CopyTextLength,
    /// A URL is empty or exceeds [`MAX_UI_OPEN_URL_BYTES`].
    OpenUrlLength,
    /// A command payload is not valid UTF-8.
    Utf8,
    /// Bytes remain after the declared command sequence.
    TrailingBytes,
}

impl fmt::Display for UiOutputDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Length => "invalid UI output length",
            Self::Magic => "invalid UI output magic",
            Self::Version => "unsupported UI output version",
            Self::Header => "invalid UI output header",
            Self::Flags => "unsupported UI output flags",
            Self::CursorIcon => "unsupported UI cursor icon",
            Self::ImeBounds => "invalid UI IME bounds",
            Self::CommandCount => "too many UI output commands",
            Self::CommandLength => "invalid UI output command length",
            Self::CommandOpcode => "unsupported UI output command",
            Self::CommandFlags => "invalid UI output command flags",
            Self::CopyTextLength => "UI clipboard text exceeds the wire limit",
            Self::OpenUrlLength => "UI URL is empty or exceeds the wire limit",
            Self::Utf8 => "UI output command is not valid UTF-8",
            Self::TrailingBytes => "UI output has trailing bytes",
        })
    }
}

impl core::error::Error for UiOutputDecodeError {}

/// Decode and validate one complete UI output v1 stream.
pub fn decode_ui_output(bytes: &[u8]) -> Result<UiOutput<'_>, UiOutputDecodeError> {
    if bytes.len() < UI_OUTPUT_HEADER_BYTES || bytes.len() > MAX_UI_OUTPUT_BYTES {
        return Err(UiOutputDecodeError::Length);
    }
    if bytes[..4] != UI_OUTPUT_MAGIC {
        return Err(UiOutputDecodeError::Magic);
    }
    if read_u16(bytes, 4) != UI_OUTPUT_VERSION {
        return Err(UiOutputDecodeError::Version);
    }
    if read_u16(bytes, 6) as usize != UI_OUTPUT_HEADER_BYTES
        || read_u32(bytes, 8) as usize != bytes.len()
    {
        return Err(UiOutputDecodeError::Header);
    }
    let command_count = read_u16(bytes, 12);
    if usize::from(command_count) > MAX_UI_OUTPUT_COMMANDS {
        return Err(UiOutputDecodeError::CommandCount);
    }
    let cursor_icon = UiCursorIcon::try_from(bytes[14])?;
    let flags = bytes[15];
    if flags & !UI_OUTPUT_FLAGS_V1 != 0 {
        return Err(UiOutputDecodeError::Flags);
    }
    let rect = UiRect {
        min_x: read_f32(bytes, 16),
        min_y: read_f32(bytes, 20),
        max_x: read_f32(bytes, 24),
        max_y: read_f32(bytes, 28),
    };
    let cursor_rect = UiRect {
        min_x: read_f32(bytes, 32),
        min_y: read_f32(bytes, 36),
        max_x: read_f32(bytes, 40),
        max_y: read_f32(bytes, 44),
    };
    let ime = if flags & UI_OUTPUT_FLAG_IME != 0 {
        let ime = UiImeOutput { rect, cursor_rect };
        if !ime.is_valid() {
            return Err(UiOutputDecodeError::ImeBounds);
        }
        Some(ime)
    } else {
        if bytes[16..UI_OUTPUT_HEADER_BYTES]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(UiOutputDecodeError::ImeBounds);
        }
        None
    };

    let mut offset = UI_OUTPUT_HEADER_BYTES;
    for _ in 0..command_count {
        let header_end = offset
            .checked_add(UI_OUTPUT_COMMAND_HEADER_BYTES)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        let header = bytes
            .get(offset..header_end)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        let opcode = header[0];
        let command_flags = header[1];
        if header[2] != 0 || header[3] != 0 {
            return Err(UiOutputDecodeError::CommandFlags);
        }
        let payload_length = read_u32(header, 4) as usize;
        let end = header_end
            .checked_add(payload_length)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        let payload = bytes
            .get(header_end..end)
            .ok_or(UiOutputDecodeError::CommandLength)?;
        match opcode {
            UI_OUTPUT_COMMAND_COPY_TEXT => {
                if command_flags != 0 {
                    return Err(UiOutputDecodeError::CommandFlags);
                }
                if payload.len() > MAX_UI_COPY_TEXT_BYTES {
                    return Err(UiOutputDecodeError::CopyTextLength);
                }
            }
            UI_OUTPUT_COMMAND_OPEN_URL => {
                if command_flags & !UI_OUTPUT_OPEN_URL_NEW_SURFACE != 0 {
                    return Err(UiOutputDecodeError::CommandFlags);
                }
                if payload.is_empty() || payload.len() > MAX_UI_OPEN_URL_BYTES {
                    return Err(UiOutputDecodeError::OpenUrlLength);
                }
            }
            _ => return Err(UiOutputDecodeError::CommandOpcode),
        }
        core::str::from_utf8(payload).map_err(|_| UiOutputDecodeError::Utf8)?;
        offset = end;
    }
    if offset != bytes.len() {
        return Err(UiOutputDecodeError::TrailingBytes);
    }

    Ok(UiOutput {
        bytes,
        descriptor: UiOutputDescriptor {
            cursor_icon,
            mutable_text_under_cursor: flags & UI_OUTPUT_FLAG_MUTABLE_TEXT != 0,
            ime,
        },
        command_count,
    })
}

fn push_u16(destination: &mut Vec<u8>, value: u16) {
    destination.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(destination: &mut Vec<u8>, value: u32) {
    destination.extend_from_slice(&value.to_le_bytes());
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("fixed field"))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed field"))
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(read_u32(bytes, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn descriptor() -> UiOutputDescriptor {
        UiOutputDescriptor {
            cursor_icon: UiCursorIcon::Text,
            mutable_text_under_cursor: true,
            ime: Some(UiImeOutput {
                rect: UiRect {
                    min_x: 10.0,
                    min_y: 20.0,
                    max_x: 210.0,
                    max_y: 60.0,
                },
                cursor_rect: UiRect {
                    min_x: 24.0,
                    min_y: 22.0,
                    max_x: 25.0,
                    max_y: 58.0,
                },
            }),
        }
    }

    #[test]
    fn output_round_trips_without_allocating_during_decode() {
        let mut bytes = Vec::new();
        let mut encoder = UiOutputEncoder::begin(&mut bytes, descriptor()).unwrap();
        encoder.copy_text("hello 🦀").unwrap();
        encoder.open_url("https://example.test/path", true).unwrap();
        encoder.finish();

        let output = decode_ui_output(&bytes).unwrap();
        assert_eq!(output.descriptor(), descriptor());
        assert_eq!(output.command_count(), 2);
        assert_eq!(
            output.commands().collect::<Vec<_>>(),
            vec![
                UiOutputCommand::CopyText("hello 🦀"),
                UiOutputCommand::OpenUrl {
                    url: "https://example.test/path",
                    new_surface: true,
                },
            ]
        );
    }

    #[test]
    fn every_cursor_value_round_trips() {
        for value in 0..=UiCursorIcon::ZoomOut as u8 {
            let mut bytes = Vec::new();
            UiOutputEncoder::begin(
                &mut bytes,
                UiOutputDescriptor {
                    cursor_icon: UiCursorIcon::try_from(value).unwrap(),
                    ..UiOutputDescriptor::default()
                },
            )
            .unwrap()
            .finish();
            assert_eq!(
                decode_ui_output(&bytes).unwrap().descriptor().cursor_icon as u8,
                value
            );
        }
    }

    #[test]
    fn malformed_headers_and_commands_are_rejected() {
        let mut bytes = Vec::new();
        UiOutputEncoder::begin(&mut bytes, UiOutputDescriptor::default())
            .unwrap()
            .finish();

        let mut invalid = bytes.clone();
        invalid[14] = 255;
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::CursorIcon
        );

        let mut invalid = bytes.clone();
        invalid[16] = 1;
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::ImeBounds
        );

        let mut invalid = bytes.clone();
        invalid[12..14].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::CommandLength
        );

        let mut invalid = bytes;
        invalid.extend_from_slice(&[UI_OUTPUT_COMMAND_COPY_TEXT, 0, 0, 0, 1, 0, 0, 0, 0xff]);
        let invalid_length = invalid.len() as u32;
        invalid[8..12].copy_from_slice(&invalid_length.to_le_bytes());
        invalid[12..14].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            decode_ui_output(&invalid).unwrap_err(),
            UiOutputDecodeError::Utf8
        );
    }

    #[test]
    fn encoder_enforces_command_limits() {
        let mut bytes = Vec::new();
        let mut encoder =
            UiOutputEncoder::begin(&mut bytes, UiOutputDescriptor::default()).unwrap();
        assert_eq!(
            encoder.open_url("", false).unwrap_err(),
            UiOutputEncodeError::OpenUrlLength
        );
        for _ in 0..MAX_UI_OUTPUT_COMMANDS {
            encoder.copy_text("").unwrap();
        }
        assert_eq!(
            encoder.copy_text("").unwrap_err(),
            UiOutputEncodeError::CommandCount
        );
    }
}
