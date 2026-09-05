/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]

use core::fmt;

/// Import name used by PolkaVM guests to read the latest motion sample.
pub const MOTION_READ_IMPORT: &str = "host_motion_read";
/// Four-byte discriminator at the start of every MotionSample v1 record.
pub const MOTION_SAMPLE_MAGIC: [u8; 4] = *b"PMO1";
/// Current MotionSample wire version.
pub const MOTION_SAMPLE_VERSION: u16 = 1;
/// Encoded byte length of MotionSample v1.
pub const MOTION_SAMPLE_BYTES: usize = 48;

/// The acceleration including gravity fields contain valid values.
pub const MOTION_FLAG_ACCELERATION: u16 = 1 << 0;
/// The rotation-rate fields contain valid values.
pub const MOTION_FLAG_ROTATION: u16 = 1 << 1;
/// Rotation was approximated from pointer movement rather than a device sensor.
pub const MOTION_FLAG_POINTER_EMULATED: u16 = 1 << 2;
/// Every flag understood by MotionSample v1.
pub const MOTION_FLAGS_V1: u16 =
    MOTION_FLAG_ACCELERATION | MOTION_FLAG_ROTATION | MOTION_FLAG_POINTER_EMULATED;

/// No motion sample has arrived since the previous successful read.
pub const MOTION_READ_NO_SAMPLE: i32 = 0;
/// The host has no motion source and no fallback implementation.
pub const MOTION_ERROR_UNAVAILABLE: i32 = -1;
/// The user or platform denied access to the motion source.
pub const MOTION_ERROR_PERMISSION_DENIED: i32 = -2;
/// The guest output range is invalid.
pub const MOTION_ERROR_INVALID_GUEST_RANGE: i32 = -3;
/// The guest output capacity is smaller than [`MOTION_SAMPLE_BYTES`].
pub const MOTION_ERROR_BUFFER_TOO_SMALL: i32 = -4;

/// Latest host motion state exposed to the guest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum MotionAvailability {
    /// No sensor or pointer fallback is available.
    Unavailable = 0,
    /// Motion reads are supported; a read can still return no newer sample.
    Available = 1,
    /// Motion exists but the user or platform denied permission.
    PermissionDenied = 2,
}

impl TryFrom<u32> for MotionAvailability {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Unavailable),
            1 => Ok(Self::Available),
            2 => Ok(Self::PermissionDenied),
            _ => Err(()),
        }
    }
}

/// One MotionSample v1 value.
///
/// Acceleration values use metres per second squared and include gravity.
/// Rotation values use degrees per second with the W3C Device Motion axes:
/// alpha around Z, beta around X, and gamma around Y. Pointer fallback fills
/// beta and gamma, sets alpha to zero, and marks
/// [`MOTION_FLAG_POINTER_EMULATED`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSample {
    /// Validity and source flags.
    pub flags: u16,
    /// Nonzero monotonically increasing host sequence.
    pub sequence: u32,
    /// Host monotonic timestamp in milliseconds.
    pub timestamp_ms: f64,
    /// Acceleration including gravity on the X axis, in m/s².
    pub acceleration_x: f32,
    /// Acceleration including gravity on the Y axis, in m/s².
    pub acceleration_y: f32,
    /// Acceleration including gravity on the Z axis, in m/s².
    pub acceleration_z: f32,
    /// Rotation rate around Z, in degrees per second.
    pub rotation_alpha: f32,
    /// Rotation rate around X, in degrees per second.
    pub rotation_beta: f32,
    /// Rotation rate around Y, in degrees per second.
    pub rotation_gamma: f32,
}

impl MotionSample {
    /// Validates and encodes this sample in the fixed little-endian v1 layout.
    pub fn encode(self) -> Result<[u8; MOTION_SAMPLE_BYTES], MotionSampleError> {
        self.validate()?;
        let mut bytes = [0u8; MOTION_SAMPLE_BYTES];
        bytes[0..4].copy_from_slice(&MOTION_SAMPLE_MAGIC);
        bytes[4..6].copy_from_slice(&MOTION_SAMPLE_VERSION.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.flags.to_le_bytes());
        bytes[8..12].copy_from_slice(&(MOTION_SAMPLE_BYTES as u32).to_le_bytes());
        bytes[12..16].copy_from_slice(&self.sequence.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.timestamp_ms.to_bits().to_le_bytes());
        for (offset, value) in [
            (24, self.acceleration_x),
            (28, self.acceleration_y),
            (32, self.acceleration_z),
            (36, self.rotation_alpha),
            (40, self.rotation_beta),
            (44, self.rotation_gamma),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
        Ok(bytes)
    }

    /// Decodes and validates a MotionSample v1 record.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionSampleError> {
        if bytes.len() != MOTION_SAMPLE_BYTES {
            return Err(MotionSampleError::Length);
        }
        if bytes[0..4] != MOTION_SAMPLE_MAGIC {
            return Err(MotionSampleError::Magic);
        }
        if read_u16(bytes, 4) != MOTION_SAMPLE_VERSION {
            return Err(MotionSampleError::Version);
        }
        if read_u32(bytes, 8) as usize != MOTION_SAMPLE_BYTES {
            return Err(MotionSampleError::Length);
        }
        let sample = Self {
            flags: read_u16(bytes, 6),
            sequence: read_u32(bytes, 12),
            timestamp_ms: f64::from_bits(read_u64(bytes, 16)),
            acceleration_x: f32::from_bits(read_u32(bytes, 24)),
            acceleration_y: f32::from_bits(read_u32(bytes, 28)),
            acceleration_z: f32::from_bits(read_u32(bytes, 32)),
            rotation_alpha: f32::from_bits(read_u32(bytes, 36)),
            rotation_beta: f32::from_bits(read_u32(bytes, 40)),
            rotation_gamma: f32::from_bits(read_u32(bytes, 44)),
        };
        sample.validate()?;
        Ok(sample)
    }

    fn validate(&self) -> Result<(), MotionSampleError> {
        if self.flags == 0 || self.flags & !MOTION_FLAGS_V1 != 0 {
            return Err(MotionSampleError::Flags);
        }
        if self.flags & MOTION_FLAG_POINTER_EMULATED != 0 && self.flags & MOTION_FLAG_ROTATION == 0
        {
            return Err(MotionSampleError::Flags);
        }
        if self.sequence == 0 {
            return Err(MotionSampleError::Sequence);
        }
        if !self.timestamp_ms.is_finite() || self.timestamp_ms < 0.0 {
            return Err(MotionSampleError::Number);
        }
        if [
            self.acceleration_x,
            self.acceleration_y,
            self.acceleration_z,
            self.rotation_alpha,
            self.rotation_beta,
            self.rotation_gamma,
        ]
        .iter()
        .any(|value| !value.is_finite())
        {
            return Err(MotionSampleError::Number);
        }
        Ok(())
    }
}

/// Standard gravitational acceleration used to normalize device tilt.
pub const STANDARD_GRAVITY_MPS2: f32 = 9.806_65;
/// Gravity projected onto one screen axis at the default full-scale tilt.
pub const TILT_GRAVITY_RANGE: f32 = 0.4;
/// Integrated pointer rotation, in degrees, that maps to full-scale tilt.
pub const POINTER_TILT_RANGE_DEGREES: f32 = 24.0;
/// Per-sample low-pass weight applied to derived tilt.
pub const TILT_SMOOTHING: f32 = 0.18;

/// Source-neutral normalized tilt derived from MotionSample v1 records.
///
/// Physical samples use acceleration including gravity relative to the first
/// observed pose. Pointer-emulated samples integrate rotation rate over their
/// monotonic timestamps. Both paths produce bounded `[-1, 1]` axes and share
/// the same low-pass filter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TiltTracker {
    baseline_gravity: Option<(f32, f32)>,
    pointer_tilt: (f32, f32),
    tilt: (f32, f32),
    last_timestamp_ms: Option<f64>,
    pointer_emulated: Option<bool>,
}

impl TiltTracker {
    /// Creates a centered tracker with no calibrated physical pose.
    pub const fn new() -> Self {
        Self {
            baseline_gravity: None,
            pointer_tilt: (0.0, 0.0),
            tilt: (0.0, 0.0),
            last_timestamp_ms: None,
            pointer_emulated: None,
        }
    }

    /// Consumes one validated sample and returns normalized horizontal and
    /// vertical tilt when that sample contains a usable source.
    pub fn update(&mut self, sample: MotionSample) -> Option<(f32, f32)> {
        let pointer_emulated = sample.flags & MOTION_FLAG_POINTER_EMULATED != 0;
        if self.pointer_emulated != Some(pointer_emulated) {
            self.last_timestamp_ms = None;
            if pointer_emulated {
                self.pointer_tilt = self.tilt;
            } else {
                self.baseline_gravity = None;
            }
            self.pointer_emulated = Some(pointer_emulated);
        }

        let target = if pointer_emulated && sample.flags & MOTION_FLAG_ROTATION != 0 {
            let elapsed_seconds = self
                .last_timestamp_ms
                .map(|previous| ((sample.timestamp_ms - previous) / 1_000.0).clamp(0.0, 0.1))
                .unwrap_or(0.0) as f32;
            self.pointer_tilt.0 = (self.pointer_tilt.0
                + sample.rotation_gamma * elapsed_seconds / POINTER_TILT_RANGE_DEGREES)
                .clamp(-1.0, 1.0);
            self.pointer_tilt.1 = (self.pointer_tilt.1
                + sample.rotation_beta * elapsed_seconds / POINTER_TILT_RANGE_DEGREES)
                .clamp(-1.0, 1.0);
            self.pointer_tilt
        } else if sample.flags & MOTION_FLAG_ACCELERATION != 0 {
            let gravity = (
                sample.acceleration_x / STANDARD_GRAVITY_MPS2,
                sample.acceleration_y / STANDARD_GRAVITY_MPS2,
            );
            let baseline = *self.baseline_gravity.get_or_insert(gravity);
            (
                (-(gravity.0 - baseline.0) / TILT_GRAVITY_RANGE).clamp(-1.0, 1.0),
                ((gravity.1 - baseline.1) / TILT_GRAVITY_RANGE).clamp(-1.0, 1.0),
            )
        } else {
            self.last_timestamp_ms = Some(sample.timestamp_ms);
            return None;
        };

        self.last_timestamp_ms = Some(sample.timestamp_ms);
        self.tilt.0 += (target.0 - self.tilt.0) * TILT_SMOOTHING;
        self.tilt.1 += (target.1 - self.tilt.1) * TILT_SMOOTHING;
        Some(self.tilt)
    }

    /// Clears calibration, integration, and smoothing state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for TiltTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// MotionSample v1 decoding or validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionSampleError {
    /// The record length or encoded length field is invalid.
    Length,
    /// The record discriminator is invalid.
    Magic,
    /// The record version is unsupported.
    Version,
    /// Flags are unknown or internally inconsistent.
    Flags,
    /// Sequence zero is reserved.
    Sequence,
    /// A floating-point field is non-finite or the timestamp is negative.
    Number,
}

impl fmt::Display for MotionSampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Length => "motion sample has an invalid length",
            Self::Magic => "motion sample has an invalid magic value",
            Self::Version => "motion sample has an unsupported version",
            Self::Flags => "motion sample has invalid flags",
            Self::Sequence => "motion sample sequence must be nonzero",
            Self::Number => "motion sample contains an invalid number",
        })
    }
}

impl core::error::Error for MotionSampleError {}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("fixed field"))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed field"))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("fixed field"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MotionSample {
        MotionSample {
            flags: MOTION_FLAG_ROTATION | MOTION_FLAG_POINTER_EMULATED,
            sequence: 7,
            timestamp_ms: 1234.5,
            acceleration_x: 0.0,
            acceleration_y: 0.0,
            acceleration_z: 0.0,
            rotation_alpha: 0.0,
            rotation_beta: -12.5,
            rotation_gamma: 8.25,
        }
    }

    #[test]
    fn motion_sample_v1_has_a_stable_golden_layout() {
        let bytes = sample().encode().unwrap();
        assert_eq!(&bytes[0..4], b"PMO1");
        assert_eq!(read_u16(&bytes, 4), 1);
        assert_eq!(read_u16(&bytes, 6), 6);
        assert_eq!(read_u32(&bytes, 8), 48);
        assert_eq!(read_u32(&bytes, 12), 7);
        assert_eq!(MotionSample::decode(&bytes), Ok(sample()));
    }

    #[test]
    fn motion_sample_v1_rejects_invalid_values() {
        let mut invalid = sample();
        invalid.sequence = 0;
        assert_eq!(invalid.encode(), Err(MotionSampleError::Sequence));
        invalid = sample();
        invalid.rotation_beta = f32::NAN;
        assert_eq!(invalid.encode(), Err(MotionSampleError::Number));
        invalid = sample();
        invalid.flags = MOTION_FLAG_POINTER_EMULATED;
        assert_eq!(invalid.encode(), Err(MotionSampleError::Flags));
    }

    #[test]
    fn tilt_tracker_calibrates_physical_gravity() {
        let mut tracker = TiltTracker::new();
        let mut physical = sample();
        physical.flags = MOTION_FLAG_ACCELERATION | MOTION_FLAG_ROTATION;
        physical.timestamp_ms = 1_000.0;
        physical.acceleration_x = 0.0;
        physical.acceleration_y = 0.0;
        physical.acceleration_z = STANDARD_GRAVITY_MPS2;
        assert_eq!(tracker.update(physical), Some((0.0, 0.0)));

        physical.sequence += 1;
        physical.timestamp_ms = 1_016.0;
        physical.acceleration_x = -STANDARD_GRAVITY_MPS2 * TILT_GRAVITY_RANGE;
        let (x, y) = tracker.update(physical).unwrap();
        assert!((x - TILT_SMOOTHING).abs() < 0.0001);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn tilt_tracker_integrates_pointer_rotation_rate() {
        let mut tracker = TiltTracker::new();
        let mut pointer = sample();
        pointer.timestamp_ms = 1_000.0;
        pointer.rotation_beta = 0.0;
        pointer.rotation_gamma = 0.0;
        assert_eq!(tracker.update(pointer), Some((0.0, 0.0)));

        pointer.sequence += 1;
        pointer.timestamp_ms = 1_100.0;
        pointer.rotation_beta = -60.0;
        pointer.rotation_gamma = 120.0;
        let (x, y) = tracker.update(pointer).unwrap();
        assert!((x - 0.09).abs() < 0.0001);
        assert!((y + 0.045).abs() < 0.0001);
    }
}
