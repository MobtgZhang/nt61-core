//! Lazily allocated stock objects with stable, validated GDI handles.
//! Stock fonts use the existing font engine; exact Win7 font metrics and the
//! per-DC DC_BRUSH/DC_PEN objects (18/19) are not implemented here.
use super::*;
use crate::ke::sync::Spinlock;
pub(super) const STOCK_FLAG: u8 = 1;
static STOCK: Spinlock<[u64; 18]> = Spinlock::new([0; 18]);

pub fn init_stock_objects() {
    for index in 0..18 {
        if index != 9 {
            let _ = gdi_get_stock_object(index);
        }
    }
}

pub fn gdi_get_stock_object(index: i32) -> u64 {
    if !(0..18).contains(&index) || index == 9 {
        return 0;
    }
    // Lock order: DC table -> stock cache -> GDI table. Deletion never takes
    // the stock lock, so publication cannot race a second stock allocation.
    let mut cache = STOCK.lock();
    if cache[index as usize] != 0 {
        return cache[index as usize];
    }
    let handle = match index {
        0 => gdi_create_solid_brush(0x00ffffff),
        1 => gdi_create_solid_brush(0x00c0c0c0),
        2 => gdi_create_solid_brush(0x00808080),
        3 => gdi_create_solid_brush(0x00404040),
        4 => gdi_create_solid_brush(0),
        5 => {
            let handle = gdi_create_solid_brush(0);
            with_handle_table(|table| {
                if let Some(entry) = table.get(handle) {
                    // SAFETY: the table lock owns this newly allocated brush.
                    unsafe {
                        (*(entry.obj_ptr as *mut GdiBrush)).style = BrushStyle::Null as u32;
                    }
                }
            });
            handle
        }
        6 => gdi_create_pen(PenStyle::Solid, 1, 0x00ffffff),
        7 => gdi_create_pen(PenStyle::Solid, 1, 0),
        8 => gdi_create_pen(PenStyle::Null, 1, 0),
        15 => gdi_create_palette(),
        10..=14 | 16..=17 => {
            let (name, height, width) = match index {
                10 => ("Terminal", 16, 8),
                11 | 16 => ("Courier", 16, 8),
                12 => ("MS Sans Serif", 16, 0),
                17 => ("MS Shell Dlg", -11, 0),
                _ => ("System", 16, 0),
            };
            let mut face = [0u16; 32];
            for (slot, byte) in face.iter_mut().zip(name.bytes()) {
                *slot = u16::from(byte);
            }
            gdi_create_font(height, width, 400, false, &face)
        }
        _ => 0,
    };
    if handle != 0 {
        with_handle_table(|table| {
            if let Some(entry) = table.get_mut(handle) {
                entry.flags |= STOCK_FLAG;
                entry.process_id = 0;
            }
        });
        cache[index as usize] = handle;
    }
    handle
}

pub extern "C" fn nt_gdi_get_stock_object(
    frame: &crate::arch::common::syscall_frame::SyscallFrame,
) -> u64 {
    gdi_get_stock_object(frame.args[0] as i32)
}
