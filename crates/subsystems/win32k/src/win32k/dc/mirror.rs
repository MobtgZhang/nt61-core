//! Win7's separately retained logical and effective horizontal window origin.
//!
//! NtGdiMirrorWindowOrg (x64 RVA 0x1fa708) updates DCATTR+0x140 from
//! logical origin +0x13c, using the device boundary and 32-bit IMUL/IDIV.
//! DC::dwSetLayout (0x1db720) changes the page mapping before invoking the
//! same calculation. Other RTL consumers, including bitmap orientation and
//! text shaping, need their own implementation before SetLayout is complete.

use super::{DcObject, PointL, MM_ANISOTROPIC};
use crate::arch::common::syscall_frame::SyscallFrame;

pub const LAYOUT_RTL: u32 = 1;

impl DcObject {
    pub(crate) fn effective_window_origin(&self) -> PointL {
        PointL::new(
            self.attributes
                .mirrored_window_origin_x
                .unwrap_or(self.window_org.x),
            self.window_org.y,
        )
    }

    pub(super) fn mirror_window_origin(&mut self) -> bool {
        if self.attributes.layout & LAYOUT_RTL == 0 {
            self.attributes.mirrored_window_origin_x = Some(self.window_org.x);
            return true;
        }
        // A zero viewport extent is a successful no-op in the original.
        if self.viewport_ext.cx == 0 {
            return true;
        }
        let product = self
            .bounds
            .right
            .wrapping_sub(self.bounds.left)
            .wrapping_sub(1)
            .wrapping_mul(self.window_ext.cx);
        let Some(offset) = product.checked_div(self.viewport_ext.cx) else {
            // The original IDIV raises #DE for MIN/-1. Do not panic inside
            // this kernel; reject that exceptional input without a write.
            return false;
        };
        self.attributes.mirrored_window_origin_x = Some(self.window_org.x.wrapping_sub(offset));
        true
    }

    pub(super) fn set_layout(&mut self, origin: i32, layout: u32) -> u32 {
        let previous = self.attributes.layout;
        let effective_origin = self.effective_window_origin().x;
        self.attributes.layout = layout;
        if (previous ^ layout) & 7 != 0 {
            if layout & LAYOUT_RTL != 0 {
                self.map_mode = MM_ANISOTROPIC;
            }
            self.window_ext.cx = self.window_ext.cx.wrapping_neg();
            self.viewport_org.x = self.viewport_org.x.wrapping_neg();
            if origin == -1 {
                self.mirror_window_origin();
            } else {
                self.attributes.mirrored_window_origin_x =
                    Some(origin.wrapping_sub(effective_origin));
            }
            if self.text_align & 6 != 6 {
                self.text_align ^= 2;
            }
        }
        previous
    }
}

pub extern "C" fn nt_gdi_mirror_window_org(frame: &SyscallFrame) -> u64 {
    super::get_dc(frame.args[0]).is_some_and(|mut dc| dc.mirror_window_origin()) as u64
}

/// Native arguments are (HDC, LONG origin, DWORD layout).
/// Kept separate from the existing stub until all RTL consumers are ready.
pub extern "C" fn nt_gdi_set_layout(frame: &SyscallFrame) -> u64 {
    super::get_dc(frame.args[0]).map_or(u32::MAX, |mut dc| {
        dc.set_layout(frame.args[1] as i32, frame.args[2] as u32)
    }) as u64
}

#[cfg(test)]
mod tests {
    use super::super::super::{clipping, objects::Rect, raster, surface::GdiSurface};
    use super::*;

    #[test]
    fn original_mirror_and_layout_instruction_fixture_matches_saved_mapping() {
        let fixture = include_bytes!("../../../tests/data/win7-mirror-window-layout.bin");
        assert_eq!(fixture.len() % 84, 0);
        assert_eq!(fixture.len() / 84, 3888);
        for (index, record) in fixture.chunks_exact(84).enumerate() {
            let word =
                |offset| u32::from_le_bytes(record[offset * 4..offset * 4 + 4].try_into().unwrap());
            let mut dc = DcObject::new();
            dc.attributes.layout = word(2);
            dc.attributes.mirrored_window_origin_x = Some(word(8) as i32);
            dc.bounds = Rect::new(word(5) as i32, 0, word(6) as i32, 30);
            dc.window_org = PointL::new(word(7) as i32, 19);
            dc.window_ext.cx = word(9) as i32;
            dc.viewport_ext.cx = word(10) as i32;
            dc.viewport_org.x = word(11) as i32;
            dc.map_mode = word(12) as i32;
            dc.text_align = word(13) as i32;
            let result = if word(1) == 0 {
                if word(0) == 0 {
                    0
                } else {
                    u32::MAX
                }
            } else if word(0) == 0 {
                u32::from(dc.mirror_window_origin())
            } else {
                dc.set_layout(word(4) as i32, word(3))
            };
            let actual = [
                result,
                dc.attributes.layout,
                dc.effective_window_origin().x as u32,
                dc.window_ext.cx as u32,
                dc.viewport_org.x as u32,
                dc.map_mode as u32,
                dc.text_align as u32,
            ];
            let expected = core::array::from_fn::<_, 7, _>(|offset| word(offset + 14));
            assert_eq!(actual, expected, "original record {index}");
            assert_eq!((dc.window_org.x, dc.window_org.y), (word(7) as i32, 19));
        }
    }

    #[test]
    fn mirrored_origin_changes_real_pixel_coordinates_and_survives_save_restore() {
        let mut surface = GdiSurface::new(8, 4, 32).unwrap();
        let mut dc = DcObject::new();
        dc.surface = (&mut surface as *mut GdiSurface) as u64;
        dc.bounds = Rect::new(0, 0, 8, 4);
        assert_eq!(dc.set_layout(-1, LAYOUT_RTL), 0);
        assert_eq!(
            (
                dc.effective_window_origin().x,
                dc.effective_window_origin().y
            ),
            (7, 0)
        );
        assert_eq!(
            clipping::logical_point(&dc, PointL::new(0, 1)).map(|p| (p.x, p.y)),
            Some((7, 1))
        );
        assert!(raster::set_pixel(&dc, 0, 1, 0x112233));
        assert!(raster::set_pixel(&dc, 1, 1, 0x445566));
        assert_eq!(surface.get_pixel(7, 1), Some(0x332211));
        assert_eq!(surface.get_pixel(6, 1), Some(0x665544));
        assert!(dc.save_state());
        dc.bounds.right = 6;
        assert!(dc.mirror_window_origin());
        assert_eq!(dc.effective_window_origin().x, 5);
        assert!(raster::set_pixel(&dc, 0, 2, 0x778899));
        assert_eq!(surface.get_pixel(5, 2), Some(0x998877));
        assert!(dc.restore_state());
        assert_eq!(dc.attributes.layout, LAYOUT_RTL);
        assert_eq!(dc.effective_window_origin().x, 7);
        assert_eq!(dc.set_layout(-1, 0), LAYOUT_RTL);
        assert_eq!(dc.window_ext.cx, 1);
        assert_eq!(dc.effective_window_origin().x, 0);
        assert!(raster::set_pixel(&dc, 0, 3, 0xabcdef));
        assert_eq!(surface.get_pixel(0, 3), Some(0xefcdab));
    }

    #[test]
    fn setting_text_mapping_temporarily_unmirrors_then_restores_layout() {
        let mut dc = DcObject::new();
        dc.bounds = Rect::new(0, 0, 8, 4);
        dc.set_layout(-1, LAYOUT_RTL);
        dc.window_ext.cx = -7;
        dc.viewport_ext.cx = 3;
        dc.viewport_org.x = -4;
        assert_eq!(
            super::super::mapping::set_map_mode(&mut dc, super::super::MM_TEXT),
            MM_ANISOTROPIC
        );
        assert_eq!(dc.map_mode, MM_ANISOTROPIC);
        assert_eq!(dc.attributes.layout, LAYOUT_RTL);
        assert_eq!(dc.window_ext.cx, -1);
        assert_eq!(dc.viewport_ext.cx, 1);
        assert_eq!(dc.viewport_org.x, -4);
        assert_eq!(dc.effective_window_origin().x, 7);
        assert_eq!(dc.text_align, 2);
    }

    #[test]
    fn mirror_zero_extent_keeps_cache_and_divide_overflow_fails_without_panicking() {
        let mut dc = DcObject::new();
        dc.attributes.layout = LAYOUT_RTL;
        dc.attributes.mirrored_window_origin_x = Some(17);
        dc.viewport_ext.cx = 0;
        dc.window_org.x = 3;
        assert!(dc.mirror_window_origin());
        assert_eq!(dc.effective_window_origin().x, 17);
        dc.bounds = Rect::new(0, 0, 2, 2);
        dc.window_ext.cx = i32::MIN;
        dc.viewport_ext.cx = -1;
        // Deliberate kernel-safe policy: original x64 IDIV raises #DE.
        assert!(!dc.mirror_window_origin());
        assert_eq!(dc.effective_window_origin().x, 17);
        dc.attributes.layout = 0;
        assert!(dc.mirror_window_origin());
        assert_eq!(dc.effective_window_origin().x, 3);
    }
}
