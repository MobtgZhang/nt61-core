use super::{Identity, SERVICES};
use crate::input_services::{
    self,
    history::{nt_user_get_mouse_move_points_ex, MouseMovePoint},
};
use crate::{call, ke, mm, ps, user_pointer};

const FAILURE: u64 = u32::MAX as u64;

pub(super) struct CallerTeb {
    words: Box<[u32; 28]>,
    previous: *mut u8,
}

impl CallerTeb {
    pub(super) fn new() -> Self {
        let mut words = Box::new([0u32; 28]);
        words[26] = 0xdeadbeef;
        let pointer = user_pointer(&mut *words, true);
        let thread = ps::thread::get_current_ethread();
        // SAFETY: the fixture supplies an exclusive per-host-thread ETHREAD.
        let previous = unsafe { core::mem::replace(&mut (*thread).kthread.teb, pointer as *mut u8) };
        Self { words, previous }
    }

    pub(super) fn error(&self) -> u32 {
        self.words[26]
    }
}

impl Drop for CallerTeb {
    fn drop(&mut self) {
        // SAFETY: restore the thread pointer before the boxed TEB is freed.
        unsafe {
            (*ps::thread::get_current_ethread()).kthread.teb = self.previous;
        }
    }
}

fn point(x: i32, y: i32, time: u32) -> MouseMovePoint {
    MouseMovePoint {
        x,
        y,
        time,
        padding: 0,
        extra_info: 0,
    }
}

fn query(input: &MouseMovePoint, output: &mut [MouseMovePoint; 64], count: i32, mode: u32) -> u64 {
    let input = user_pointer(input, false);
    let output = user_pointer(output, true);
    call(
        nt_user_get_mouse_move_points_ex,
        &[24, input, output, count as u64, mode as u64],
    )
}

#[test]
fn history_wraps_at_sixty_four_and_retains_duplicate_positions_and_timestamps() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(951, 951);
    let teb = CallerTeb::new();
    assert_eq!(core::mem::size_of::<MouseMovePoint>(), 24);
    assert_eq!(core::mem::offset_of!(MouseMovePoint, extra_info), 16);
    for index in 0..67 {
        ke::time::TICKS.with(|tick| tick.set(index + 10));
        assert!(input_services::set_cursor(index as i32, index as i32 * 2));
    }
    let mut output = [MouseMovePoint::default(); 64];
    assert_eq!(query(&point(66, 132, 0), &mut output, 64, 1), 64);
    assert_eq!(teb.error(), 0xdeadbeef);
    for (index, sample) in output.iter().enumerate() {
        assert_eq!(
            (sample.x, sample.y),
            (66 - index as i32, 132 - index as i32 * 2)
        );
        assert_eq!(sample.time, 76 - index as u32);
        assert_eq!(sample.padding, 0);
        assert_eq!(sample.extra_info, 0);
    }
    assert_eq!(query(&point(2, 4, 0), &mut output, 64, 1), FAILURE);
    assert_eq!(teb.error(), 1171);

    for time in [100, 101] {
        ke::time::TICKS.with(|tick| tick.set(time));
        assert!(input_services::set_cursor(7, 9));
    }
    assert_eq!(query(&point(7, 9, 0), &mut output, 64, 1), 64);
    assert_eq!((output[0].time, output[1].time), (101, 100));
    assert_eq!(query(&point(7, 9, 100), &mut output, 64, 1), 63);
    assert_eq!(output[0].time, 100);
}

#[test]
fn history_is_shared_between_callers_and_accepts_aliased_buffers() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(952, 952);
    for index in 0..64 {
        assert!(input_services::set_cursor(200 + index, 300));
    }
    let _foreign = Identity::enter(953, 953);
    let mut points = [MouseMovePoint::default(); 64];
    points[0] = point(263, 300, 0);
    let pointer = user_pointer(&mut points, true);
    assert_eq!(
        call(
            nt_user_get_mouse_move_points_ex,
            &[24, pointer, pointer, 64, 1]
        ),
        64
    );
    assert_eq!((points[0].x, points[63].x), (263, 200));
}

#[test]
fn hardware_history_preserves_absolute_precision_and_respects_blocked_input() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(954, 954);
    let mut output = [MouseMovePoint::default(); 64];
    ke::time::TICKS.with(|tick| tick.set(u32::MAX));
    input_services::position_hardware_cursor_with_resolution(100, 200, Some((6410, 17100)));
    assert_eq!(query(&point(100, 200, u32::MAX), &mut output, 1, 2), 1);
    assert_eq!((output[0].x, output[0].y), (6410, 17100));
    ke::time::TICKS.with(|tick| tick.set(0));
    assert_eq!(input_services::move_hardware_cursor(1, 2), (101, 202));
    assert_eq!(query(&point(101, 202, 0), &mut output, 2, 1), 2);
    assert_eq!((output[0].time, output[1].time), (0, u32::MAX));

    assert_eq!(call(input_services::nt_user_block_input, &[1]), 1);
    assert_eq!(input_services::move_hardware_cursor(100, 100), (101, 202));
    assert_eq!(
        input_services::position_hardware_cursor(500, 600),
        (101, 202)
    );
    assert_eq!(query(&point(101, 202, 0), &mut output, 2, 1), 2);
    assert_eq!((output[0].time, output[1].time), (0, u32::MAX));
    assert_eq!(call(input_services::nt_user_block_input, &[0]), 1);
    assert!(input_services::set_cursor(1023, 767));
    assert_eq!(query(&point(1023, 767, 0), &mut output, 1, 2), 1);
    assert_eq!((output[0].x, output[0].y), (65535, 65535));
}

#[test]
fn history_validates_size_count_buffers_and_zero_length_queries() {
    let _serial = SERVICES.lock().unwrap();
    let _identity = Identity::enter(955, 955);
    let teb = CallerTeb::new();
    for (size, count) in [(0, 1), (23, 1), (25, 1), (24, -1), (24, 65)] {
        assert_eq!(
            call(
                nt_user_get_mouse_move_points_ex,
                &[size, 0, 0, count as u64, 1]
            ),
            FAILURE
        );
        assert_eq!(teb.error(), 87);
    }
    assert_eq!(
        call(nt_user_get_mouse_move_points_ex, &[24, 0, 0, 1, 1]),
        FAILURE
    );
    assert_eq!(teb.error(), 998);
    let input = point(10, 11, 0);
    assert!(input_services::set_cursor(input.x, input.y));
    let pointer = user_pointer(&input, false);
    assert_eq!(
        call(nt_user_get_mouse_move_points_ex, &[24, pointer, 0, 1, 1]),
        FAILURE
    );
    assert_eq!(teb.error(), 998);
    assert_eq!(
        call(nt_user_get_mouse_move_points_ex, &[24, pointer, 0, 0, 1]),
        0
    );
    assert_eq!(teb.error(), 998);
    let mut output = [MouseMovePoint::default(); 64];
    assert_eq!(query(&input, &mut output, 0, 9), FAILURE);
    assert_eq!(teb.error(), 87);
    // A read-only output region must not be written, even for a valid point.
    let readonly = [0xccu8; 24];
    let readonly_pointer = user_pointer(&readonly, false);
    assert_eq!(
        call(
            nt_user_get_mouse_move_points_ex,
            &[24, pointer, readonly_pointer, 1, 1]
        ),
        FAILURE
    );
    assert_eq!(teb.error(), 998);
    assert_eq!(readonly, [0xcc; 24]);
    // An input that straddles an allowed range must fail capture.
    let short = [0u8; 8];
    let pointer = short.as_ptr() as u64;
    mm::user_copy::allow(pointer, short.len(), false);
    assert_eq!(
        call(nt_user_get_mouse_move_points_ex, &[24, pointer, 0, 0, 1]),
        FAILURE
    );
    assert_eq!(teb.error(), 998);
}

#[test]
fn history_errors_are_written_only_to_the_calling_threads_teb() {
    let _serial = SERVICES.lock().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = [87, 998]
        .into_iter()
        .map(|expected| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let teb = CallerTeb::new();
                let size = if expected == 87 { 0 } else { 24 };
                assert_eq!(
                    call(nt_user_get_mouse_move_points_ex, &[size, 0, 0, 1, 1]),
                    FAILURE
                );
                barrier.wait();
                assert_eq!(teb.error(), expected);
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}
