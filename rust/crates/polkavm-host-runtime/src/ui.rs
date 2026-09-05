use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::INPUT_EVENT_BYTES;

pub const MAX_UI_TEXT_BYTES: usize = 4 * 1024;
pub const MAX_UI_SEMANTICS_BYTES: usize = 256 * 1024;
pub const MAX_UI_SEMANTIC_NODES: usize = 1_024;
pub const MAX_UI_SEMANTIC_STRING_BYTES: usize = 1_024;

pub const INPUT_TEXT_COMMIT: u8 = 8;
pub const INPUT_IME_PREEDIT: u8 = 9;
pub const INPUT_IME_COMMIT: u8 = 10;
pub const INPUT_IME_ENABLED: u8 = 11;
pub const INPUT_IME_DISABLED: u8 = 12;
pub const INPUT_FOCUS: u8 = 13;
pub const INPUT_WHEEL: u8 = 14;
pub const INPUT_POINTER_CAPTURE: u8 = 15;

const CHUNK_LENGTH_MASK: u8 = 0x07;
const CHUNK_FIRST: u8 = 0x40;
const CHUNK_LAST: u8 = 0x80;
const CHUNK_ALLOWED: u8 = CHUNK_LENGTH_MASK | CHUNK_FIRST | CHUNK_LAST;
const CHUNK_BYTES: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputKind {
    Text,
    ImePreedit,
    ImeCommit,
}

impl TextInputKind {
    fn event_type(self) -> u8 {
        match self {
            Self::Text => INPUT_TEXT_COMMIT,
            Self::ImePreedit => INPUT_IME_PREEDIT,
            Self::ImeCommit => INPUT_IME_COMMIT,
        }
    }
}

pub fn encode_text_input(kind: TextInputKind, text: &str) -> Result<Vec<[u8; INPUT_EVENT_BYTES]>> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_UI_TEXT_BYTES {
        bail!("UI text exceeds {MAX_UI_TEXT_BYTES} bytes");
    }
    let chunks = bytes.len().max(1).div_ceil(CHUNK_BYTES);
    let mut records = Vec::with_capacity(chunks);
    for index in 0..chunks {
        let start = index * CHUNK_BYTES;
        let end = bytes.len().min(start + CHUNK_BYTES);
        let chunk = &bytes[start..end];
        let mut record = [0u8; INPUT_EVENT_BYTES];
        record[0] = kind.event_type();
        record[1] = u8::try_from(chunk.len()).unwrap();
        if index == 0 {
            record[1] |= CHUNK_FIRST;
        }
        if index + 1 == chunks {
            record[1] |= CHUNK_LAST;
        }
        record[2..2 + chunk.len()].copy_from_slice(chunk);
        records.push(record);
    }
    Ok(records)
}

pub fn ime_state_record(enabled: bool) -> [u8; INPUT_EVENT_BYTES] {
    let mut record = [0u8; INPUT_EVENT_BYTES];
    record[0] = if enabled {
        INPUT_IME_ENABLED
    } else {
        INPUT_IME_DISABLED
    };
    record
}

pub fn focus_record(focused: bool) -> [u8; INPUT_EVENT_BYTES] {
    let mut record = [0u8; INPUT_EVENT_BYTES];
    record[0] = INPUT_FOCUS;
    record[1] = u8::from(focused);
    record
}

pub fn wheel_record(delta_x: i16, delta_y: i16) -> [u8; INPUT_EVENT_BYTES] {
    let mut record = [0u8; INPUT_EVENT_BYTES];
    record[0] = INPUT_WHEEL;
    record[2..4].copy_from_slice(&delta_x.to_le_bytes());
    record[4..6].copy_from_slice(&delta_y.to_le_bytes());
    record
}

/// Announces that the Host started or ended pointer capture, including capture
/// the user ended with the platform escape affordance.
pub fn pointer_capture_record(active: bool) -> [u8; INPUT_EVENT_BYTES] {
    let mut record = [0u8; INPUT_EVENT_BYTES];
    record[0] = INPUT_POINTER_CAPTURE;
    record[1] = u8::from(active);
    record
}

pub(crate) fn validate_input_record(record: &[u8; INPUT_EVENT_BYTES]) -> Result<()> {
    match record[0] {
        1..=7 => {
            if record[6..] != [0, 0] {
                bail!("fixed input record has nonzero reserved bytes");
            }
        }
        INPUT_TEXT_COMMIT | INPUT_IME_PREEDIT | INPUT_IME_COMMIT => {
            if record[1] & !CHUNK_ALLOWED != 0 {
                bail!("text input record has invalid flags");
            }
            let length = usize::from(record[1] & CHUNK_LENGTH_MASK);
            if length > CHUNK_BYTES || record[2 + length..].iter().any(|byte| *byte != 0) {
                bail!("text input record has invalid padding");
            }
        }
        INPUT_IME_ENABLED | INPUT_IME_DISABLED => {
            if record[1..].iter().any(|byte| *byte != 0) {
                bail!("IME state record has nonzero payload");
            }
        }
        INPUT_FOCUS => {
            if record[1] > 1 || record[2..].iter().any(|byte| *byte != 0) {
                bail!("focus record is malformed");
            }
        }
        INPUT_WHEEL => {
            if record[1] != 0 || record[6..] != [0, 0] {
                bail!("wheel record is malformed");
            }
        }
        INPUT_POINTER_CAPTURE => {
            if record[1] > 1 || record[2..].iter().any(|byte| *byte != 0) {
                bail!("pointer capture record is malformed");
            }
        }
        _ => bail!("unsupported input record type {}", record[0]),
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiSemanticRole {
    Window,
    Group,
    Label,
    Button,
    Link,
    CheckBox,
    Slider,
    TextInput,
    MultilineTextInput,
    PasswordInput,
    Image,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiSemanticAction {
    Click,
    Focus,
    SetValue,
    Increment,
    Decrement,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UiSemanticNode {
    pub id: String,
    pub parent: Option<String>,
    pub role: UiSemanticRole,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
    pub bounds: [f32; 4],
    #[serde(default)]
    pub actions: Vec<UiSemanticAction>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct UiSemanticSnapshot {
    pub version: u32,
    pub generation: u64,
    pub nodes: Vec<UiSemanticNode>,
}

#[derive(Clone, Debug)]
pub struct UiSemanticsFrame {
    pub bytes: Vec<u8>,
}

pub(crate) fn validate_ui_semantics(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_UI_SEMANTICS_BYTES {
        bail!("UI semantics must contain 1..={MAX_UI_SEMANTICS_BYTES} bytes");
    }
    let snapshot: UiSemanticSnapshot = serde_json::from_slice(bytes)
        .map_err(|error| anyhow!("invalid UI semantics JSON: {error}"))?;
    if snapshot.version != 1 || snapshot.nodes.is_empty() {
        bail!("UI semantics must contain a version 1 node tree");
    }
    if snapshot.nodes.len() > MAX_UI_SEMANTIC_NODES {
        bail!("UI semantics exceed {MAX_UI_SEMANTIC_NODES} nodes");
    }
    let mut ids = HashSet::with_capacity(snapshot.nodes.len());
    let mut roots = 0usize;
    for node in &snapshot.nodes {
        if !valid_semantic_id(&node.id) || !ids.insert(node.id.clone()) {
            bail!("UI semantics contain an invalid or duplicate node id");
        }
        if node.parent.is_none() {
            roots += 1;
        }
        if node.name.len() > MAX_UI_SEMANTIC_STRING_BYTES
            || node.value.len() > MAX_UI_SEMANTIC_STRING_BYTES
        {
            bail!("UI semantic node string exceeds {MAX_UI_SEMANTIC_STRING_BYTES} bytes");
        }
        let [x0, y0, x1, y1] = node.bounds;
        if !node.bounds.iter().all(|value| value.is_finite()) || x1 < x0 || y1 < y0 {
            bail!("UI semantic node has invalid bounds");
        }
    }
    if roots != 1 {
        bail!("UI semantics must contain exactly one root");
    }
    for node in &snapshot.nodes {
        if node
            .parent
            .as_ref()
            .is_some_and(|parent| !ids.contains(parent))
        {
            bail!("UI semantic node references an unknown parent");
        }
    }
    Ok(())
}

fn valid_semantic_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_records_round_trip_chunk_boundaries() {
        let records = encode_text_input(TextInputKind::Text, "hello π").unwrap();
        assert_eq!(records.len(), 2);
        assert_ne!(records[0][1] & CHUNK_FIRST, 0);
        assert_ne!(records[1][1] & CHUNK_LAST, 0);
        for record in &records {
            validate_input_record(record).unwrap();
        }
    }

    #[test]
    fn semantics_require_one_bounded_tree() {
        let valid = serde_json::to_vec(&UiSemanticSnapshot {
            version: 1,
            generation: 7,
            nodes: vec![UiSemanticNode {
                id: "1".into(),
                parent: None,
                role: UiSemanticRole::Window,
                name: "Playground".into(),
                value: String::new(),
                bounds: [0.0, 0.0, 640.0, 480.0],
                actions: Vec::new(),
                disabled: false,
                focused: false,
            }],
        })
        .unwrap();
        validate_ui_semantics(&valid).unwrap();

        let malformed = br#"{"version":1,"generation":1,"nodes":[]}"#;
        assert!(validate_ui_semantics(malformed).is_err());
    }
}
