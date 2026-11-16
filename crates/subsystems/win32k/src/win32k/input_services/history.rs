//! Bounded pointer history for the current interactive window station.
//! Hardware producers allocate nothing. Query output is copied after releasing
//! the input lock, and input/output buffers may alias.

use super::{screen_rect, INPUT};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::mm::user_copy::{copy_to_user, probe_user_write, read_user_object};

const CAPACITY: usize = 64;
const POINT_SIZE: usize = 24;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_NOACCESS: u32 = 998;
const ERROR_POINT_NOT_FOUND: u32 = 1171;

/// NT64 MOUSEMOVEPOINT, shared by all four supported 64-bit ports.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseMovePoint {
    pub x: i32,
    pub y: i32,
    pub time: u32,
    pub padding: u32,
    pub extra_info: u64,
}

#[derive(Clone, Copy)]
struct Sample {
    display: MouseMovePoint,
    high: (u16, u16),
}

pub(super) struct MoveHistory {
    samples: [Option<Sample>; CAPACITY],
    next: usize,
    length: usize,
}

impl MoveHistory {
    pub(super) const fn new() -> Self {
        Self {
            samples: [None; CAPACITY],
            next: 0,
            length: 0,
        }
    }

    pub(super) fn record(
        &mut self,
        position: (i32, i32),
        high: Option<(u16, u16)>,
        extra_info: u64,
    ) {
        let high = high.unwrap_or_else(|| {
            let screen = screen_rect();
            let scale = |value: i32, origin: i32, end: i32| {
                let span = (i64::from(end) - i64::from(origin) - 1).max(1);
                ((i64::from(value) - i64::from(origin)).clamp(0, span) * 65535 / span) as u16
            };
            (
                scale(position.0, screen.left, screen.right),
                scale(position.1, screen.top, screen.bottom),
            )
        });
        self.samples[self.next] = Some(Sample {
            display: MouseMovePoint {
                x: position.0 & 0xffff,
                y: position.1 & 0xffff,
                time: crate::ke::time::get_tick_count(),
                padding: 0,
                extra_info,
            },
            high,
        });
        self.next = (self.next + 1) % CAPACITY;
        self.length = (self.length + 1).min(CAPACITY);
    }

    fn query(
        &self,
        input: MouseMovePoint,
        output: &mut [MouseMovePoint],
        high: bool,
    ) -> Option<usize> {
        let mut found = false;
        let mut count = 0;
        for age in 0..self.length {
            let sample = self.samples[(self.next + CAPACITY - 1 - age) % CAPACITY]?;
            if !found {
                // The search key is always in display coordinates, regardless
                // of the requested output resolution. Nonzero time disambiguates
                // repeated positions, including around the 32-bit tick wrap.
                found = sample.display.x == input.x
                    && sample.display.y == input.y
                    && (input.time == 0 || input.time == sample.display.time);
                if !found {
                    continue;
                }
            }
            if count == output.len() {
                break;
            }
            let mut point = sample.display;
            if high {
                point.x = i32::from(sample.high.0);
                point.y = i32::from(sample.high.1);
            }
            output[count] = point;
            count += 1;
        }
        found.then_some(count)
    }
}

fn fail(error: u32) -> u64 {
    super::set_last_error(error);
    u32::MAX as u64
}

pub extern "C" fn nt_user_get_mouse_move_points_ex(frame: &SyscallFrame) -> u64 {
    let count = frame.args[3] as i32;
    if frame.args[0] as u32 != POINT_SIZE as u32 || !(0..=CAPACITY as i32).contains(&count) {
        return fail(ERROR_INVALID_PARAMETER);
    }
    if frame.args[1] == 0 || (count != 0 && frame.args[2] == 0) {
        return fail(ERROR_NOACCESS);
    }
    let Ok(input) = read_user_object::<MouseMovePoint>(frame.args[1]) else {
        return fail(ERROR_NOACCESS);
    };
    let resolution = frame.args[4] as u32;
    if !matches!(resolution, 1 | 2) {
        return fail(ERROR_INVALID_PARAMETER);
    }
    if count != 0 && probe_user_write(frame.args[2], count as usize * POINT_SIZE).is_err() {
        return fail(ERROR_NOACCESS);
    }
    let mut output = [MouseMovePoint::default(); CAPACITY];
    let result = INPUT
        .lock()
        .history
        .query(input, &mut output[..count as usize], resolution == 2);
    let Some(count) = result else {
        return fail(ERROR_POINT_NOT_FOUND);
    };
    if count != 0 {
        // All fields, including ABI padding, are explicitly initialized above.
        // Serialize instead of reading Rust struct padding into caller memory.
        let mut bytes = [0u8; CAPACITY * POINT_SIZE];
        for (point, bytes) in output[..count]
            .iter()
            .zip(bytes.chunks_exact_mut(POINT_SIZE))
        {
            bytes[..4].copy_from_slice(&point.x.to_le_bytes());
            bytes[4..8].copy_from_slice(&point.y.to_le_bytes());
            bytes[8..12].copy_from_slice(&point.time.to_le_bytes());
            bytes[16..24].copy_from_slice(&point.extra_info.to_le_bytes());
        }
        if copy_to_user(frame.args[2], &bytes[..count * POINT_SIZE]).is_err() {
            return fail(ERROR_NOACCESS);
        }
    }
    count as u64
}
