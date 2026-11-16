//! Execute production GDI objects, geometry, and native region handlers on the
//! host. Only pool allocation, locking, framebuffer discovery, and user memory are adapted.
extern crate alloc;

#[path = "../src/win32k/window.rs"]
pub mod window;
#[path = "../src/win32k/user_state.rs"]
pub mod user_state;
mod menu_service_cases;
mod window_cases;

#[path = "../src/win32k/clipboard.rs"]
pub mod clipboard;
#[path = "clipboard_cases.rs"]
mod clipboard_cases;

#[path = "../src/win32k/font_services.rs"]
pub mod font_services;
#[path = "font_cases.rs"]
mod font_cases;
mod text_drawing_cases;

#[path = "../src/win32k/bitmap.rs"]
pub mod bitmap;

#[path = "bitmap_cases.rs"]
mod bitmap_cases;

#[path = "path_cases.rs"]
mod path_cases;

#[path = "mapping_cases.rs"]
mod mapping_cases;

#[path = "dc_query_cases.rs"]
mod dc_query_cases;
#[path = "palette_cases.rs"]
mod palette_cases;
#[path = "brush_cases.rs"]
mod brush_cases;
#[path = "curve_region_cases.rs"]
mod curve_region_cases;
#[path = "shape_cases.rs"]
mod shape_cases;
#[path = "bounds_cases.rs"]
mod bounds_cases;
#[path = "raster_effect_cases.rs"]
mod raster_effect_cases;
mod stretch_cases;
mod alpha_cases;
mod mask_cases;
mod parallelogram_cases;
mod dib_cases;
mod dib_readback_cases;
mod dib_device_cases;
mod pattern_cases;
#[path = "../src/win32k/palette.rs"]
pub mod palette;

#[path = "../src/win32k/objects.rs"]
pub mod objects;
#[path = "../src/win32k/region.rs"]
pub mod region;
#[path = "../src/win32k/region_geometry.rs"]
pub mod region_geometry;

#[path = "../src/win32k/clipping.rs"]
pub mod clipping;
#[path = "../src/win32k/dc.rs"]
pub mod dc;
#[path = "../src/win32k/gdi_ops.rs"]
pub mod gdi_ops;
#[path = "../src/win32k/raster.rs"]
pub mod raster;
#[path = "../src/win32k/selection.rs"]
pub mod selection;
#[path = "../src/win32k/surface.rs"]
pub mod surface;
#[path = "../src/win32k/transforms.rs"]
pub mod transforms;

#[path = "../src/win32k/path.rs"]
pub mod path;
#[path = "../src/win32k/polygon.rs"]
pub mod polygon;

mod libs {
    pub mod win32k {
        pub use crate::{clipboard, dc, input_services, objects, surface, window};
        pub mod message {
            pub use crate::message::{MessageQueue, Msg};
            pub static POSTED: std::sync::Mutex<Vec<(u64, u32, u64, i64)>> = std::sync::Mutex::new(Vec::new());
            pub fn post_thread_message(_: u64, hwnd: u64, message: u32, wparam: u64, lparam: i64) -> bool {
                post_message(hwnd, message, wparam, lparam)
            }
            pub fn post_message(hwnd: u64, message: u32, wparam: u64, lparam: i64) -> bool {
                POSTED.lock().unwrap().push((hwnd, message, wparam, lparam));
                true
            }
        }
    }
}

mod hal {
    pub mod common {
        pub mod framebuffer {
            #[derive(Default)]
            pub struct Info {
                pub width: u32,
                pub height: u32,
                pub bpp: u32,
                pub pitch: u32,
                pub address: usize,
            }
            pub fn info() -> Info {
                Info::default()
            }
        }
    }
}

mod ke {
    pub mod scheduler {
        pub fn yield_() { std::thread::yield_now(); }
    }
    pub mod time {
        std::thread_local! { pub static TICKS: core::cell::Cell<u32> = const { core::cell::Cell::new(1) }; }
        pub fn get_tick_count() -> u32 { TICKS.with(|ticks| ticks.get()) }
    }
    pub mod sync {
        pub struct Spinlock<T>(std::sync::Mutex<T>);
        // Kernel tables contain pool pointers; their production guard owns access.
        unsafe impl<T> Sync for Spinlock<T> {}
        pub type SpinlockGuard<'a, T> = std::sync::MutexGuard<'a, T>;
        impl<T> Spinlock<T> {
            pub const fn new(value: T) -> Self {
                Self(std::sync::Mutex::new(value))
            }
            pub fn lock(&self) -> SpinlockGuard<'_, T> {
                self.0.lock().unwrap()
            }
        }
    }
}

mod arch {
    pub mod common {
        pub mod percpu {
            pub fn get_current_thread() -> *mut crate::ps::thread::Thread {
                crate::ps::thread::get_current_ethread()
            }
        }
        pub mod syscall_frame {
            #[derive(Default)]
            pub struct SyscallFrame {
                pub args: [u64; 17],
            }
        }
    }
}

mod mm {
    pub mod pool {
        use std::cell::Cell;
        use std::{
            alloc::{alloc_zeroed, dealloc, Layout},
            collections::BTreeMap,
            sync::{LazyLock, Mutex},
        };
        static ALLOCATIONS: LazyLock<Mutex<BTreeMap<usize, Layout>>> =
            LazyLock::new(|| Mutex::new(BTreeMap::new()));
        thread_local! {
            static FAIL_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
            static BALANCE: Cell<isize> = const { Cell::new(0) };
        }
        pub fn fail_after(successes: usize) {
            FAIL_AFTER.set(Some(successes));
        }
        pub fn balance() -> isize {
            BALANCE.get()
        }
        pub enum PoolType {
            NonPaged,
        }
        pub fn allocate(_: PoolType, size: usize) -> *mut u8 {
            if FAIL_AFTER.with(|remaining| match remaining.get() {
                Some(0) => { remaining.set(None); true }
                Some(count) => { remaining.set(Some(count - 1)); false }
                None => false,
            }) {
                return core::ptr::null_mut();
            }
            let layout = Layout::from_size_align(size, 16).unwrap();
            let pointer = unsafe { alloc_zeroed(layout) };
            if pointer.is_null() {
                return pointer;
            }
            BALANCE.set(BALANCE.get() + 1);
            ALLOCATIONS.lock().unwrap().insert(pointer as usize, layout);
            pointer
        }
        pub unsafe fn free(pointer: *mut u8) -> Result<(), ()> {
            let layout = ALLOCATIONS
                .lock()
                .unwrap()
                .remove(&(pointer as usize))
                .expect("double free");
            unsafe {
                dealloc(pointer, layout);
            }
            BALANCE.set(BALANCE.get() - 1);
            Ok(())
        }
    }
    pub mod user_copy {
        use std::cell::RefCell;
        thread_local! { static RANGES: RefCell<Vec<(u64, usize, bool)>> = const { RefCell::new(Vec::new()) }; }
        pub fn allow(pointer: u64, size: usize, writable: bool) {
            RANGES.with(|ranges| ranges.borrow_mut().push((pointer, size, writable)));
        }
        fn probe(address: u64, size: usize, write: bool) -> Result<(), i32> {
            RANGES.with(|ranges| {
                if address != 0
                    && ranges.borrow().iter().any(|&(base, length, writable)| {
                        (!write || writable)
                            && address >= base
                            && address
                                .checked_add(size as u64)
                                .is_some_and(|end| end <= base + length as u64)
                    })
                {
                    Ok(())
                } else {
                    Err(0xc0000005u32 as i32)
                }
            })
        }
        pub fn probe_user_read(address: u64, size: usize) -> Result<(), i32> {
            probe(address, size, false)
        }
        pub fn probe_user_write(address: u64, size: usize) -> Result<(), i32> {
            probe(address, size, true)
        }
        pub fn copy_from_user(dst: &mut [u8], src: u64) -> Result<(), i32> {
            probe(src, dst.len(), false)?;
            unsafe {
                core::ptr::copy_nonoverlapping(src as *const u8, dst.as_mut_ptr(), dst.len());
            }
            Ok(())
        }
        pub fn copy_to_user(dst: u64, src: &[u8]) -> Result<(), i32> {
            probe(dst, src.len(), true)?;
            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr(), dst as *mut u8, src.len());
            }
            Ok(())
        }
        pub fn read_user_object<T: Copy>(src: u64) -> Result<T, i32> {
            probe(src, core::mem::size_of::<T>(), false)?;
            Ok(unsafe { core::ptr::read_unaligned(src as *const T) })
        }
        pub fn write_user_object<T: Copy>(dst: u64, value: &T) -> Result<(), i32> {
            probe(dst, core::mem::size_of::<T>(), true)?;
            unsafe {
                core::ptr::write_unaligned(dst as *mut T, *value);
            }
            Ok(())
        }
    }
}

use arch::common::syscall_frame::SyscallFrame;
use objects::Rect;
use region::services::*;
use region_geometry::{Region, RegionCombineMode as Mode};

fn call(service: extern "C" fn(&SyscallFrame) -> u64, arguments: &[u64]) -> u64 {
    let mut frame = SyscallFrame::default();
    frame.args[..arguments.len()].copy_from_slice(arguments);
    service(&frame)
}

fn create(rect: [i32; 4]) -> u64 {
    let handle = call(nt_gdi_create_rect_rgn, &rect.map(|n| n as u64));
    assert_ne!(handle, 0);
    handle
}

fn bounds(handle: u64) -> (u64, Rect) {
    let mut rect = Rect::new(-1, -1, -1, -1);
    let pointer = &mut rect as *mut Rect as u64;
    mm::user_copy::allow(pointer, 16, true);
    (call(nt_gdi_get_rgn_box, &[handle, pointer]), rect)
}

#[test]
fn empty_reversed_and_negative_rectangles_are_real_distinct_objects() {
    let empty = create([5, 5, 5, 20]);
    let reversed = create([10, 20, -10, -20]);
    assert_ne!(empty, reversed);
    assert_eq!(bounds(empty), (1, Rect::default()));
    assert_eq!(bounds(reversed), (2, Rect::new(-10, -20, 10, 20)));
    assert_eq!(
        call(
            nt_gdi_pt_in_region,
            &[reversed, (-10i32) as u64, (-20i32) as u64]
        ),
        1
    );
    assert_eq!(call(nt_gdi_pt_in_region, &[reversed, 10, 0]), 0);
    assert!(objects::gdi_delete_object(empty));
    assert!(objects::gdi_delete_object(reversed));
}

#[test]
fn difference_retains_holes_and_in_place_operations_are_alias_safe() {
    let outer = create([0, 0, 20, 20]);
    let inner = create([5, 5, 15, 15]);
    assert_eq!(call(nt_gdi_combine_rgn, &[outer, outer, inner, 4]), 3);
    assert_eq!(call(nt_gdi_pt_in_region, &[outer, 10, 10]), 0);
    assert_eq!(call(nt_gdi_pt_in_region, &[outer, 1, 10]), 1);
    assert_eq!(bounds(outer), (3, Rect::new(0, 0, 20, 20)));
    assert_eq!(call(nt_gdi_combine_rgn, &[outer, outer, outer, 3]), 1);
    assert_eq!(bounds(outer), (1, Rect::default()));
    assert!(objects::gdi_delete_object(outer));
    assert!(objects::gdi_delete_object(inner));
}

#[test]
fn all_boolean_modes_match_independent_pixel_sets_for_small_rectangles() {
    for left in -2..=2 {
        for top in -2..=2 {
            for right in left..=3 {
                let first = Region::rectangle(Rect::new(left, top, right, 3)).unwrap();
                let second = Region::rectangle(Rect::new(-1, -1, 2, 2)).unwrap();
                for mode in [
                    Mode::Intersection,
                    Mode::Union,
                    Mode::Xor,
                    Mode::Difference,
                    Mode::Copy,
                ] {
                    let result = first.combine(&second, mode).unwrap();
                    for y in -3..=4 {
                        for x in -3..=4 {
                            let a = first.contains_point(x, y);
                            let b = second.contains_point(x, y);
                            let expected = match mode {
                                Mode::Intersection => a && b,
                                Mode::Union => a || b,
                                Mode::Xor => a != b,
                                Mode::Difference => a && !b,
                                Mode::Copy => a,
                            };
                            assert_eq!(result.contains_point(x, y), expected, "{mode:?} ({x},{y})");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn canonical_bands_coalesce_and_equality_compares_shape() {
    let whole = Region::rectangle(Rect::new(0, 0, 20, 20)).unwrap();
    let pieces = Region::from_rectangles(&[
        Rect::new(10, 10, 20, 20),
        Rect::new(0, 0, 10, 20),
        Rect::new(10, 0, 20, 10),
    ])
    .unwrap();
    assert_eq!(whole, pieces);
    let hole = whole
        .combine(
            &Region::rectangle(Rect::new(5, 5, 15, 15)).unwrap(),
            Mode::Difference,
        )
        .unwrap();
    assert_ne!(whole, hole);
    assert!(!hole.intersects_rect(&Rect::new(6, 6, 14, 14)));
    assert!(hole.intersects_rect(&Rect::new(4, 6, 14, 14)));
}

#[test]
fn invalid_mode_wrong_type_and_missing_source_do_not_mutate_destination() {
    let destination = create([1, 2, 3, 4]);
    let pen = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0);
    for arguments in [
        [destination, destination, pen, 1],
        [destination, pen, 0, 5],
        [destination, destination, 0, 6],
        [pen, destination, 0, 5],
    ] {
        assert_eq!(call(nt_gdi_combine_rgn, &arguments), 0);
        assert_eq!(bounds(destination), (2, Rect::new(1, 2, 3, 4)));
    }
    assert_eq!(
        call(nt_gdi_combine_rgn, &[destination, destination, u64::MAX, 5]),
        2
    );
    assert!(objects::gdi_delete_object(destination));
    assert!(objects::gdi_delete_object(pen));
}

#[test]
fn offset_checks_all_coordinates_before_commit_and_rejects_overflow() {
    let handle = create([-4, -3, 10, 20]);
    assert_eq!(call(nt_gdi_offset_rgn, &[handle, (-10i32) as u64, 5]), 2);
    let expected = bounds(handle);
    assert_eq!(expected, (2, Rect::new(-14, 2, 0, 25)));
    assert_eq!(call(nt_gdi_offset_rgn, &[handle, i32::MAX as u64, 0]), 0);
    assert_eq!(bounds(handle), expected);
    assert_eq!(
        call(nt_gdi_set_rect_rgn, &[handle, 0, 0, i32::MAX as u64, 1]),
        0
    );
    assert_eq!(bounds(handle), expected);
    assert!(objects::gdi_delete_object(handle));
}

#[test]
fn region_data_round_trip_has_native_layout_and_bounded_writes() {
    let handle = create([0, 0, 20, 20]);
    let hole = create([5, 5, 15, 15]);
    assert_eq!(call(nt_gdi_combine_rgn, &[handle, handle, hole, 4]), 3);
    let required = call(nt_gdi_get_region_data, &[handle, 0, 0]);
    assert_eq!(required, 32 + 4 * 16);
    let mut data = vec![0xa5u8; required as usize + 16];
    let pointer = data.as_mut_ptr() as u64;
    mm::user_copy::allow(pointer, data.len(), true);
    assert_eq!(
        call(nt_gdi_get_region_data, &[handle, required - 1, pointer]),
        0
    );
    assert!(data.iter().all(|&byte| byte == 0xa5));
    assert_eq!(
        call(nt_gdi_get_region_data, &[handle, required, pointer]),
        required
    );
    assert_eq!(
        &data[..16],
        &[32, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 64, 0, 0, 0]
    );
    assert!(data[required as usize..].iter().all(|&byte| byte == 0xa5));
    let imported = call(nt_gdi_ext_create_region, &[0, required, pointer]);
    assert_ne!(imported, 0);
    assert_eq!(call(nt_gdi_equal_rgn, &[handle, imported]), 1);
    for invalid_offset in [0usize, 4, 8] {
        let original = data[invalid_offset];
        data[invalid_offset] = 0xff;
        assert_eq!(call(nt_gdi_ext_create_region, &[0, required, pointer]), 0);
        data[invalid_offset] = original;
    }
    for object in [handle, hole, imported] {
        assert!(objects::gdi_delete_object(object));
    }
}

#[test]
fn invalid_user_buffers_and_invalid_handles_never_receive_output() {
    let handle = create([1, 2, 3, 4]);
    assert_eq!(call(nt_gdi_get_rgn_box, &[handle, 0]), 0);
    assert_eq!(call(nt_gdi_get_rgn_box, &[handle, u64::MAX]), 0);
    assert_eq!(call(nt_gdi_rect_in_region, &[handle, u64::MAX]), 0);
    assert_eq!(call(nt_gdi_get_region_data, &[handle, 48, u64::MAX]), 0);
    assert_eq!(
        call(nt_gdi_ext_create_region, &[0, u32::MAX as u64, u64::MAX]),
        0
    );
    assert!(objects::gdi_delete_object(handle));
    assert_eq!(bounds(handle), (0, Rect::new(-1, -1, -1, -1)));
}

#[test]
fn handle_indices_match_allocations_and_stale_or_forged_handles_fail() {
    let mut table = objects::GdiHandleTable::new();
    let first = table.allocate(objects::GdiObjectType::Region, 0x1234, 10);
    assert_eq!(table.get(first).unwrap().obj_ptr, 0x1234);
    let forged = first ^ (1 << 16);
    assert!(table.free(forged).is_none());
    assert!(table.get(first).is_some());
    assert!(table.free(first).is_some());
    let next = table.allocate(objects::GdiObjectType::Region, 0x5678, 10);
    assert_ne!(first, next);
    assert!(table.get(first).is_none());
    assert!(table.get(first | (1 << 63)).is_none());
    assert_eq!(table.get(next).unwrap().obj_ptr, 0x5678);
}

#[test]
fn concurrent_region_mutation_and_deletion_do_not_use_freed_allocations() {
    let handle = create([0, 0, 20, 20]);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..10_000 {
                    let _ = region::gre_offset_rgn(handle, 0, 0);
                }
            });
        }
        barrier.wait();
        assert!(objects::gdi_delete_object(handle));
    });
    assert!(!objects::gdi_delete_object(handle));
    assert_eq!(region::gre_offset_rgn(handle, 0, 0), 0);
}

struct DrawingFixture {
    handle: u64,
    surface: Box<surface::GdiSurface>,
}

impl DrawingFixture {
    fn new() -> Self {
        let mut surface =
            Box::new(surface::GdiSurface::new(16, 16, surface::PIXEL_FORMAT_32BPP).unwrap());
        surface.clear(0x00123456);
        let handle = dc::get_dc_table().create(2, &mut *surface as *mut _ as u64);
        assert_ne!(handle, 0);
        Self { handle, surface }
    }
    fn pixel(&self, x: i32, y: i32) -> u32 {
        self.surface.get_pixel(x, y).unwrap()
    }
}
impl Drop for DrawingFixture {
    fn drop(&mut self) {
        assert!(dc::gre_delete_dc(self.handle));
    }
}

#[test]
fn clip_selection_copies_geometry_and_survives_source_mutation_and_deletion() {
    let fixture = DrawingFixture::new();
    let clip = create([2, 3, 9, 10]);
    assert_eq!(
        call(
            clipping::nt_gdi_ext_select_clip_rgn,
            &[fixture.handle, clip, 5]
        ),
        2
    );
    assert_eq!(call(nt_gdi_set_rect_rgn, &[clip, 0, 0, 1, 1]), 1);
    assert!(objects::gdi_delete_object(clip));
    assert_eq!(
        call(clipping::nt_gdi_pt_visible, &[fixture.handle, 3, 4]),
        1
    );
    assert_eq!(
        call(clipping::nt_gdi_pt_visible, &[fixture.handle, 0, 0]),
        0
    );
    let destination = create([0, 0, 0, 0]);
    assert_eq!(
        call(
            clipping::nt_gdi_get_random_rgn,
            &[fixture.handle, destination, 1]
        ),
        1
    );
    assert_eq!(bounds(destination), (2, Rect::new(2, 3, 9, 10)));
    assert_eq!(
        call(
            clipping::nt_gdi_ext_select_clip_rgn,
            &[fixture.handle, 0, 1]
        ),
        0
    );
    assert_eq!(
        call(
            clipping::nt_gdi_ext_select_clip_rgn,
            &[fixture.handle, 0, 5]
        ),
        2
    );
    assert_eq!(
        call(
            clipping::nt_gdi_get_random_rgn,
            &[fixture.handle, destination, 1]
        ),
        0
    );
    assert_eq!(bounds(destination), (2, Rect::new(2, 3, 9, 10)));
    assert!(objects::gdi_delete_object(destination));
}

#[test]
fn meta_clip_intersects_application_and_cannot_be_removed_by_clearing_application() {
    let fixture = DrawingFixture::new();
    assert_eq!(
        call(
            clipping::nt_gdi_intersect_clip_rect,
            &[fixture.handle, 1, 2, 12, 13]
        ),
        3
    );
    assert_eq!(call(clipping::nt_gdi_set_meta_rgn, &[fixture.handle]), 2);
    assert_eq!(
        call(
            clipping::nt_gdi_exclude_clip_rect,
            &[fixture.handle, 3, 4, 8, 9]
        ),
        3
    );
    assert_eq!(
        call(clipping::nt_gdi_pt_visible, &[fixture.handle, 5, 5]),
        0
    );
    assert_eq!(
        call(
            clipping::nt_gdi_ext_select_clip_rgn,
            &[fixture.handle, 0, 5]
        ),
        2
    );
    assert_eq!(
        call(clipping::nt_gdi_pt_visible, &[fixture.handle, 5, 5]),
        1
    );
    assert_eq!(
        call(clipping::nt_gdi_pt_visible, &[fixture.handle, 0, 0]),
        0
    );
    let mut rect = Rect::default();
    let pointer = &mut rect as *mut _ as u64;
    mm::user_copy::allow(pointer, 16, true);
    assert_eq!(
        call(
            clipping::nt_gdi_get_app_clip_box,
            &[fixture.handle, pointer]
        ),
        2
    );
    assert_eq!(rect, Rect::new(1, 2, 12, 13));
}

#[test]
fn clipping_and_get_clip_box_apply_translated_scaled_coordinates() {
    let fixture = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.viewport_org = dc::PointL::new(2, 2);
        dc.viewport_ext = dc::SizeL::new(2, 2);
    }
    assert_eq!(
        call(
            clipping::nt_gdi_intersect_clip_rect,
            &[fixture.handle, 1, 1, 4, 4]
        ),
        3
    );
    let result = create([0, 0, 0, 0]);
    assert_eq!(
        call(
            clipping::nt_gdi_get_random_rgn,
            &[fixture.handle, result, 1]
        ),
        1
    );
    assert_eq!(bounds(result), (2, Rect::new(4, 4, 10, 10)));
    let mut rect = Rect::default();
    let pointer = &mut rect as *mut _ as u64;
    mm::user_copy::allow(pointer, 16, true);
    assert_eq!(
        call(
            clipping::nt_gdi_get_app_clip_box,
            &[fixture.handle, pointer]
        ),
        2
    );
    assert_eq!(rect, Rect::new(1, 1, 4, 4));
    assert_eq!(
        call(clipping::nt_gdi_offset_clip_rgn, &[fixture.handle, 1, 1]),
        2
    );
    assert_eq!(
        call(
            clipping::nt_gdi_get_random_rgn,
            &[fixture.handle, result, 1]
        ),
        1
    );
    assert_eq!(bounds(result), (2, Rect::new(6, 6, 12, 12)));
    assert!(objects::gdi_delete_object(result));
}

#[test]
fn save_restore_levels_restore_owned_clip_current_position_and_transform() {
    let fixture = DrawingFixture::new();
    assert_eq!(
        call(
            clipping::nt_gdi_intersect_clip_rect,
            &[fixture.handle, 1, 1, 15, 15]
        ),
        3
    );
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        assert!(gdi_ops::gre_move_to(&mut dc, -300, 400));
        dc.miter_limit = 7.0;
    }
    assert_eq!(dc::gre_save_dc(fixture.handle), 1);
    assert_eq!(
        call(
            clipping::nt_gdi_exclude_clip_rect,
            &[fixture.handle, 5, 5, 10, 10]
        ),
        3
    );
    assert_eq!(dc::gre_save_dc(fixture.handle), 2);
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.cur_x = 12;
        dc.viewport_org.x = 3;
        dc.miter_limit = 2.0;
    }
    for invalid in [0, 3, -3, i32::MIN, i32::MAX] {
        assert!(!dc::gre_restore_dc(fixture.handle, invalid));
        assert_eq!(dc::get_dc(fixture.handle).unwrap().cur_x, 12);
    }
    assert!(dc::gre_restore_dc(fixture.handle, -1));
    assert_eq!(
        call(clipping::nt_gdi_pt_visible, &[fixture.handle, 7, 7]),
        0
    );
    {
        let dc = dc::get_dc(fixture.handle).unwrap();
        assert_eq!((dc.cur_x, dc.cur_y, dc.viewport_org.x), (-300, 400, 0));
        assert_eq!(dc.miter_limit, 7.0);
    }
    assert!(dc::gre_restore_dc(fixture.handle, 1));
    assert_eq!(
        call(clipping::nt_gdi_pt_visible, &[fixture.handle, 7, 7]),
        1
    );
    assert!(!dc::gre_restore_dc(fixture.handle, -1));
}

#[test]
fn clipped_fill_and_invert_change_only_region_pixels_and_preserve_holes() {
    let fixture = DrawingFixture::new();
    let region = create([1, 1, 14, 14]);
    let hole = create([4, 4, 10, 10]);
    assert_eq!(call(nt_gdi_combine_rgn, &[region, region, hole, 4]), 3);
    assert_eq!(
        call(
            clipping::nt_gdi_intersect_clip_rect,
            &[fixture.handle, 2, 2, 12, 12]
        ),
        3
    );
    let brush = objects::gdi_create_solid_brush(0x000000ff); // COLORREF red
    assert_eq!(
        call(raster::nt_gdi_fill_rgn, &[fixture.handle, region, brush]),
        1
    );
    for y in 0..16 {
        for x in 0..16 {
            let painted = (2..12).contains(&x)
                && (2..12).contains(&y)
                && !((4..10).contains(&x) && (4..10).contains(&y));
            assert_eq!(
                fixture.pixel(x, y),
                if painted { 0x00ff0000 } else { 0x00123456 }
            );
        }
    }
    assert_eq!(
        call(raster::nt_gdi_invert_rgn, &[fixture.handle, region]),
        1
    );
    assert_eq!(fixture.pixel(2, 2), 0x0000ffff);
    assert_eq!(fixture.pixel(5, 5), 0x00123456);
    assert_eq!(fixture.pixel(0, 0), 0x00123456);
    assert_eq!(
        call(raster::nt_gdi_fill_rgn, &[fixture.handle, region, hole]),
        0
    );
    for handle in [region, hole, brush] {
        assert!(objects::gdi_delete_object(handle));
    }
}

#[test]
fn set_get_pixel_use_bound_dc_clipping_colorref_and_invalid_color() {
    let first = DrawingFixture::new();
    let second = DrawingFixture::new();
    assert_eq!(
        call(raster::nt_gdi_set_pixel, &[first.handle, 3, 4, 0x00bb33aa]),
        0x00bb33aa
    );
    assert_eq!(first.pixel(3, 4), 0x00aa33bb);
    assert_eq!(second.pixel(3, 4), 0x00123456);
    assert_eq!(
        call(raster::nt_gdi_get_pixel, &[first.handle, 3, 4]),
        0x00bb33aa
    );
    assert_eq!(
        call(
            clipping::nt_gdi_exclude_clip_rect,
            &[first.handle, 2, 2, 5, 5]
        ),
        3
    );
    assert_eq!(
        call(raster::nt_gdi_set_pixel, &[first.handle, 3, 4, 0]),
        u32::MAX as u64
    );
    assert_eq!(
        call(raster::nt_gdi_get_pixel, &[first.handle, 3, 4]),
        u32::MAX as u64
    );
    for invalid in [0, 1, 0x7fffffff, u64::MAX] {
        assert_eq!(
            call(raster::nt_gdi_set_pixel, &[invalid, 0, 0, 0]),
            u32::MAX as u64
        );
        assert_eq!(
            call(clipping::nt_gdi_intersect_clip_rect, &[invalid, 0, 0, 1, 1]),
            0
        );
    }
}

#[test]
fn all_256_rop3_tables_match_independent_per_bit_truth_table() {
    let destination = 0x0056789a;
    let source = 0x00ab3fe1;
    let pattern = 0x00c97426;
    for table in 0u32..=255 {
        let mut expected = 0;
        for bit in 0..24 {
            let index = (((pattern >> bit) & 1) << 2)
                | (((source >> bit) & 1) << 1)
                | ((destination >> bit) & 1);
            expected |= ((table >> index) & 1) << bit;
        }
        assert_eq!(
            gdi_ops::apply_rop(destination, source, pattern, table << 16),
            expected
        );
    }
}

#[test]
fn overlapping_clipped_blit_captures_source_before_any_write() {
    let mut fixture = DrawingFixture::new();
    for y in 0..16 {
        for x in 0..16 {
            fixture.surface.set_pixel(x, y, (y * 16 + x) as u32);
        }
    }
    assert_eq!(
        call(
            clipping::nt_gdi_exclude_clip_rect,
            &[fixture.handle, 6, 0, 8, 16]
        ),
        3
    );
    let dc = dc::get_dc(fixture.handle).unwrap();
    assert!(raster::bit_blt(
        &dc,
        2,
        2,
        12,
        12,
        Some(&dc),
        0,
        0,
        gdi_ops::SRCCOPY
    ));
    for y in 0..16 {
        for x in 0..16 {
            let copied = (2..14).contains(&x) && (2..14).contains(&y) && !(6..8).contains(&x);
            let expected = if copied {
                (y - 2) * 16 + x - 2
            } else {
                y * 16 + x
            };
            assert_eq!(fixture.pixel(x, y), expected as u32, "({x},{y})");
        }
    }
}

#[test]
fn blits_with_negative_origins_crop_safely_and_source_clipping_is_ignored() {
    let mut source = DrawingFixture::new();
    let destination = DrawingFixture::new();
    source.surface.clear(0x00aabbcc);
    assert_eq!(
        call(
            clipping::nt_gdi_intersect_clip_rect,
            &[source.handle, 0, 0, 0, 0]
        ),
        1
    );
    let mut table = dc::get_dc_table();
    let source_surface = table.get(source.handle).unwrap().surface;
    let mut snapshot = dc::DcObject::new();
    snapshot.surface = source_surface;
    let dc = table.get(destination.handle).unwrap();
    assert!(gdi_ops::gre_bit_blt(
        dc,
        -2,
        -3,
        6,
        7,
        Some(&snapshot),
        -1,
        -1,
        gdi_ops::SRCCOPY
    ));
    assert_eq!(destination.pixel(0, 0), 0x00aabbcc);
    assert_eq!(destination.pixel(3, 3), 0x00aabbcc);
    assert_eq!(destination.pixel(4, 4), 0x00123456);
    assert!(!gdi_ops::gre_bit_blt(
        dc,
        i32::MAX,
        0,
        2,
        1,
        Some(&snapshot),
        0,
        0,
        gdi_ops::SRCCOPY
    ));
}

#[test]
fn concurrent_dc_deletion_waits_for_guarded_access_without_stale_lookup() {
    let handle = dc::get_dc_table().create(2, 0);
    let guard = dc::get_dc(handle).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        let deleted = dc::gre_delete_dc(handle);
        finished_tx.send(deleted).unwrap();
    });
    started_rx.recv().unwrap();
    assert!(finished_rx.try_recv().is_err());
    assert_eq!(guard.cur_x, 0);
    drop(guard);
    assert!(finished_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap());
    worker.join().unwrap();
    assert!(dc::get_dc(handle).is_none());
    assert!(!dc::gre_delete_dc(handle));
}

#[test]
fn line_to_respects_clip_excludes_end_and_restores_logical_position() {
    let fixture = DrawingFixture::new();
    let pen = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0x000000ff);
    assert_eq!(
        call(
            clipping::nt_gdi_exclude_clip_rect,
            &[fixture.handle, 4, 0, 7, 16]
        ),
        3
    );
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.pen = pen;
        assert!(gdi_ops::gre_move_to(&mut dc, -2, 3));
    }
    assert_eq!(dc::gre_save_dc(fixture.handle), 1);
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        assert!(gdi_ops::gre_line_to(&mut dc, 12, 3));
        assert_eq!((dc.cur_x, dc.cur_y), (12, 3));
    }
    for x in 0..16 {
        assert_eq!(
            fixture.pixel(x, 3),
            if x < 12 && !(4..7).contains(&x) {
                0x00ff0000
            } else {
                0x00123456
            }
        );
    }
    assert!(dc::gre_restore_dc(fixture.handle, -1));
    assert_eq!(dc::get_dc(fixture.handle).unwrap().cur_x, -2);
    assert!(!objects::gdi_delete_object(pen));
    drop(fixture);
    assert!(objects::gdi_delete_object(pen));
}

#[test]
fn rectangle_keeps_border_separate_from_interior_and_applies_rop2_once() {
    let fixture = DrawingFixture::new();
    let pen = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0x000000ff);
    let brush = objects::gdi_create_solid_brush(0x0000ff00);
    assert_eq!(
        call(
            clipping::nt_gdi_exclude_clip_rect,
            &[fixture.handle, 6, 0, 8, 16]
        ),
        3
    );
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.pen = pen;
        dc.brush = brush;
        assert!(gdi_ops::gre_rectangle(&mut dc, 2, 2, 12, 12));
    }
    assert_eq!(fixture.pixel(2, 2), 0x00ff0000);
    assert_eq!(fixture.pixel(11, 11), 0x00ff0000);
    assert_eq!(fixture.pixel(3, 3), 0x0000ff00);
    assert_eq!(fixture.pixel(6, 6), 0x00123456);
    assert_eq!(fixture.pixel(12, 12), 0x00123456);
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.rop_mode = 7; // R2_XORPEN
        assert!(gdi_ops::gre_rectangle(&mut dc, 2, 2, 12, 12));
    }
    assert_eq!(fixture.pixel(2, 2), 0);
    assert_eq!(fixture.pixel(3, 3), 0);
    for handle in [pen, brush] {
        assert!(objects::gdi_delete_object(handle));
    }
}

#[test]
fn packed_surface_rows_are_dword_aligned_and_reject_unsupported_pixel_access() {
    for (width, format, pitch) in [(1, 1, 4), (33, 1, 8), (9, 4, 8), (3, 24, 12), (5, 32, 20)] {
        let mut surface = surface::GdiSurface::new(width, -3, format).unwrap();
        assert_eq!((surface.height, surface.pitch), (3, pitch));
        if format != 32 {
            assert_eq!(surface.get_pixel(0, 0), None);
            assert!(!surface.set_pixel(0, 0, 0));
        }
    }
    for (width, height, format) in [(0, 1, 32), (1, i32::MIN, 32), (i32::MAX, 1, 32), (1, 1, 99)] {
        assert!(surface::GdiSurface::new(width, height, format).is_none());
    }
}

fn user_pointer<T>(value: *const T, writable: bool) -> u64 {
    let pointer = value as *const T as u64;
    mm::user_copy::allow(pointer, core::mem::size_of::<T>(), writable);
    pointer
}

#[test]
fn transform_composition_preserves_translation_order_and_supports_aliases() {
    use dc::Xform;
    let first = Xform {
        m11: 2.0,
        m22: 3.0,
        dx: 5.0,
        dy: 7.0,
        ..Xform::IDENTITY
    };
    let second = Xform {
        m11: 0.0,
        m12: 1.0,
        m21: -1.0,
        m22: 0.0,
        dx: 11.0,
        dy: 13.0,
    };
    let mut output = first;
    let output_ptr = user_pointer(&mut output, true);
    assert_eq!(
        call(
            transforms::nt_gdi_combine_transform,
            &[output_ptr, output_ptr, user_pointer(&second, false)]
        ),
        1
    );
    assert_eq!(
        [output.m11, output.m12, output.m21, output.m22, output.dx, output.dy],
        [0.0, 2.0, -3.0, 0.0, 4.0, 18.0]
    );
    let point = dc::PointL::new(4, -2);
    let result = transforms::apply(output, point).unwrap();
    let sequential = transforms::apply(second, transforms::apply(first, point).unwrap()).unwrap();
    assert_eq!((result.x, result.y), (sequential.x, sequential.y));
    let back = transforms::apply(transforms::inverse(output).unwrap(), result).unwrap();
    assert_eq!((back.x, back.y), (point.x, point.y));
    let invalid = Xform {
        dx: f32::NAN,
        ..Xform::IDENTITY
    };
    assert_eq!(
        call(
            transforms::nt_gdi_combine_transform,
            &[
                output_ptr,
                user_pointer(&invalid, false),
                user_pointer(&second, false)
            ]
        ),
        0
    );
    assert_eq!(output.dx, 4.0);
}

#[test]
fn native_world_transform_modes_are_ordered_and_identity_accepts_null() {
    use dc::Xform;
    let fixture = DrawingFixture::new();
    let scale = Xform {
        m11: 2.0,
        m22: 3.0,
        dx: 5.0,
        dy: 7.0,
        ..Xform::IDENTITY
    };
    let shift = Xform {
        dx: 11.0,
        dy: 13.0,
        ..Xform::IDENTITY
    };
    let scale_ptr = user_pointer(&scale, false);
    let shift_ptr = user_pointer(&shift, false);
    assert_eq!(
        call(
            transforms::nt_gdi_modify_world_transform,
            &[fixture.handle, scale_ptr, 4]
        ),
        1
    );
    assert_eq!(
        call(
            transforms::nt_gdi_modify_world_transform,
            &[fixture.handle, shift_ptr, 2]
        ),
        1
    );
    assert_eq!(dc::get_dc(fixture.handle).unwrap().world_transform.dx, 27.0);
    assert_eq!(
        call(
            transforms::nt_gdi_modify_world_transform,
            &[fixture.handle, scale_ptr, 4]
        ),
        1
    );
    assert_eq!(
        call(
            transforms::nt_gdi_modify_world_transform,
            &[fixture.handle, shift_ptr, 3]
        ),
        1
    );
    assert_eq!(dc::get_dc(fixture.handle).unwrap().world_transform.dx, 16.0);
    assert_eq!(
        call(
            transforms::nt_gdi_modify_world_transform,
            &[fixture.handle, 0xdeadbeef, 1]
        ),
        0
    );
    assert_eq!(dc::get_dc(fixture.handle).unwrap().world_transform.dx, 16.0);
    assert_eq!(
        call(
            transforms::nt_gdi_modify_world_transform,
            &[fixture.handle, 0, 1]
        ),
        1
    );
    assert_eq!(dc::get_dc(fixture.handle).unwrap().world_transform.dx, 0.0);
    assert_eq!(
        call(
            transforms::nt_gdi_modify_world_transform,
            &[fixture.handle, 0, 4]
        ),
        0
    );
}

#[test]
fn native_point_arrays_transform_in_place_and_failure_preserves_output() {
    let fixture = DrawingFixture::new();
    {
        let mut dc = dc::get_dc(fixture.handle).unwrap();
        dc.viewport_ext = dc::SizeL::new(2, -3);
        dc.viewport_org = dc::PointL::new(10, 20);
        dc.window_org = dc::PointL::new(1, 2);
    }
    let mut points = [dc::PointL::new(4, 5), dc::PointL::new(-3, -2)];
    let pointer = user_pointer(&mut points, true);
    assert_eq!(
        call(
            transforms::nt_gdi_transform_points,
            &[fixture.handle, pointer, pointer, 2, 1]
        ),
        1
    );
    assert_eq!(
        (points[0].x, points[0].y, points[1].x, points[1].y),
        (16, 11, 2, 32)
    );
    assert_eq!(
        call(
            transforms::nt_gdi_transform_points,
            &[fixture.handle, pointer, pointer, 2, 0]
        ),
        1
    );
    assert_eq!(
        (points[0].x, points[0].y, points[1].x, points[1].y),
        (4, 5, -3, -2)
    );
    assert_eq!(
        call(
            transforms::nt_gdi_transform_points,
            &[fixture.handle, 0xdeadbeef, pointer, 2, 1]
        ),
        0
    );
    assert_eq!(points[0].x, 4);
    dc::get_dc(fixture.handle).unwrap().world_transform.m11 = 0.0;
    assert_eq!(
        call(
            transforms::nt_gdi_transform_points,
            &[fixture.handle, pointer, pointer, 2, 0]
        ),
        0
    );
    assert_eq!(points[0].x, 4);
    let mut output = dc::Xform::IDENTITY;
    assert_eq!(
        call(
            transforms::nt_gdi_get_transform,
            &[fixture.handle, 0x402, user_pointer(&mut output, true)]
        ),
        0
    );
    assert_eq!(output.m11, 1.0);
    assert_eq!(
        call(transforms::nt_gdi_transform_points, &[0, 0, 0, 0, 99]),
        1
    );
}

#[test]
fn native_dc_point_and_move_to_return_structures_and_restore_position() {
    let fixture = DrawingFixture::new();
    let mut point = dc::PointL::new(-1, -1);
    let pointer = user_pointer(&mut point, true);
    assert_eq!(
        call(
            transforms::nt_gdi_get_dc_point,
            &[fixture.handle, 1, pointer]
        ),
        1
    );
    assert_eq!((point.x, point.y), (1, 1));
    assert_eq!(
        call(
            transforms::nt_gdi_get_dc_point,
            &[fixture.handle, 99, pointer]
        ),
        0
    );
    assert_eq!(
        call(
            transforms::nt_gdi_get_dc_point,
            &[fixture.handle, 1, 0xdeadbeef]
        ),
        0
    );
    assert_eq!(
        call(transforms::nt_gdi_move_to, &[fixture.handle, 3, 4, pointer]),
        1
    );
    assert_eq!((point.x, point.y), (0, 0));
    assert_eq!(dc::gre_save_dc(fixture.handle), 1);
    assert_eq!(
        call(transforms::nt_gdi_move_to, &[fixture.handle, 8, 9, 0]),
        1
    );
    assert!(dc::gre_restore_dc(fixture.handle, -1));
    let dc = dc::get_dc(fixture.handle).unwrap();
    assert_eq!((dc.cur_x, dc.cur_y), (3, 4));
}

#[test]
fn native_selection_validates_type_and_saved_states_retain_objects() {
    let fixture = DrawingFixture::new();
    let first = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0);
    let second = objects::gdi_create_pen(objects::PenStyle::Solid, 1, 0xffffff);
    let brush = objects::gdi_create_solid_brush(0x123456);
    assert_eq!(
        call(selection::nt_gdi_select_pen, &[fixture.handle, first]),
        objects::gdi_get_stock_object(7)
    );
    assert_eq!(dc::get_dc(fixture.handle).unwrap().pen, first);
    assert!(!objects::gdi_delete_object(first));
    assert_eq!(dc::gre_save_dc(fixture.handle), 1);
    assert_eq!(
        call(selection::nt_gdi_select_pen, &[fixture.handle, second]),
        first
    );
    assert!(!objects::gdi_delete_object(first)); // Saved state owns its reference.
    assert_eq!(
        call(selection::nt_gdi_select_pen, &[fixture.handle, brush]),
        0
    );
    assert_eq!(
        call(
            selection::nt_gdi_select_pen,
            &[fixture.handle, first | 1 << 40]
        ),
        0
    );
    assert_eq!(dc::get_dc(fixture.handle).unwrap().pen, second);
    assert!(dc::gre_restore_dc(fixture.handle, -1));
    assert_eq!(dc::get_dc(fixture.handle).unwrap().pen, first);
    assert!(objects::gdi_delete_object(second));
    assert_eq!(
        call(selection::nt_gdi_select_brush, &[fixture.handle, brush]),
        objects::gdi_get_stock_object(0)
    );
    assert!(!objects::gdi_delete_object(brush));
    drop(fixture);
    assert!(objects::gdi_delete_object(first));
    assert!(objects::gdi_delete_object(brush));
}

#[test]
fn discarded_saved_states_and_dc_deletion_release_every_selection_reference() {
    let fixture = DrawingFixture::new();
    let pens: Vec<_> = (0..3)
        .map(|color| objects::gdi_create_pen(objects::PenStyle::Solid, 1, color))
        .collect();
    let font = objects::gdi_create_font(12, 0, 400, false, &[65, 0]);
    for (index, &pen) in pens.iter().enumerate() {
        call(selection::nt_gdi_select_pen, &[fixture.handle, pen]);
        assert_eq!(dc::gre_save_dc(fixture.handle), index as i32 + 1);
    }
    assert_eq!(
        call(selection::nt_gdi_select_font, &[fixture.handle, font]),
        objects::gdi_get_stock_object(13)
    );
    assert!(dc::gre_restore_dc(fixture.handle, 1));
    assert!(objects::gdi_delete_object(font));
    assert!(objects::gdi_delete_object(pens[1]));
    assert!(objects::gdi_delete_object(pens[2]));
    assert!(!objects::gdi_delete_object(pens[0]));
    drop(fixture);
    assert!(objects::gdi_delete_object(pens[0]));
}

#[test]
fn ext_create_region_applies_axis_aligned_transform_and_rejects_shear() {
    let source = create([1, 2, 4, 6]);
    let size = call(nt_gdi_get_region_data, &[source, 0, 0]);
    let mut data = vec![0u8; size as usize];
    let data_pointer = data.as_mut_ptr() as u64;
    mm::user_copy::allow(data_pointer, data.len(), true);
    assert_eq!(
        call(nt_gdi_get_region_data, &[source, size, data_pointer]),
        size
    );
    let mut transform = dc::Xform {
        m11: -2.0,
        m22: 3.0,
        dx: 10.0,
        dy: -5.0,
        ..dc::Xform::IDENTITY
    };
    let pointer = user_pointer(&transform, false);
    let output = call(nt_gdi_ext_create_region, &[pointer, size, data_pointer]);
    assert_ne!(output, 0);
    assert_eq!(bounds(output), (2, Rect::new(2, 1, 8, 13)));
    transform.m21 = 0.5;
    let pointer = user_pointer(&transform, false);
    assert_eq!(
        call(nt_gdi_ext_create_region, &[pointer, size, data_pointer]),
        0
    );
    assert!(objects::gdi_delete_object(source));
    assert!(objects::gdi_delete_object(output));
}

mod ps {
    pub mod process {
        pub fn gui_resource_process(handle: u64) -> Option<u64> {
            (handle == u64::MAX).then(|| super::thread::IDENTITY.with(|id| id.get().0))
        }
    }
    pub mod thread {
        pub struct ClientId { pub unique_process: u64, pub unique_thread: u64 }
        pub struct Kthread { pub teb: *mut u8 }
        pub struct Thread { pub client_id: ClientId, pub kthread: Kthread }
        std::thread_local! {
            pub static IDENTITY: core::cell::Cell<(u64, u64)> = const { core::cell::Cell::new((1, 1)) };
            static THREAD: core::cell::UnsafeCell<Thread> = const { core::cell::UnsafeCell::new(Thread { client_id: ClientId { unique_process: 1, unique_thread: 1 }, kthread: Kthread { teb: core::ptr::null_mut() } }) };
        }
        pub fn find_thread_by_tid(tid: u64) -> Option<*mut Thread> {
            (tid == IDENTITY.with(|id| id.get().1)).then(get_current_ethread)
        }
        pub fn get_current_ethread() -> *mut Thread {
            THREAD.with(|thread| {
                let pointer = thread.get();
                let (process, id) = IDENTITY.with(|value| value.get());
                // SAFETY: the fixture only accesses its calling thread's cell.
                unsafe { (*pointer).client_id = ClientId { unique_process: process, unique_thread: id }; }
                pointer
            })
        }
    }
}

mod input_context_cases;

#[path = "../src/win32k/input_services.rs"]
pub mod input_services;
#[path = "../src/win32k/raw_input.rs"]
pub mod raw_input;

// Exercise the production queue, including cross-thread delivery and WM_INPUT
// cleanup; the existing window fixtures retain their lightweight transport.
#[path = "../src/win32k/message.rs"]
pub mod message;
#[path = "../src/win32k/display_config.rs"]
pub mod display_config;
#[path = "../src/win32k/color_space.rs"]
pub mod color_space;

#[path = "../src/win32k/resources.rs"]
pub mod resources;

#[path = "native_service_cases.rs"]
mod native_service_cases;

// Execute in-tree user32 input wrappers against production USER services.
#[path = "../../../libs/user32/src/user32/input.rs"]
pub mod user32_input;
#[path = "../../../libs/user32/src/user32/types.rs"]
pub mod types;
pub mod user32_message_fixture;
