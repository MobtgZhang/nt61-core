//! Information DC transitions over the existing display device.
//!
//! Win7 DC::bMakeInfoDC detaches its drawing surface without destroying it.
//! Display-owned bindings in this backend are borrowed, including while
//! detached. SaveDC stores attributes; it cannot reattach this lifetime state.
//!
//! Evidence: Win7 SP1 x64 NtGdiMakeInfoDC RVA 0x205630 and
//! DC::bMakeInfoDC RVA 0x23a98c. GreSaveDC/GreRestoreDC temporarily disable
//! information mode while manipulating saved attributes, then re-enable the
//! mode that was active on entry. Their saved state does not change that mode.

use super::super::{objects::Rect, surface::GdiSurface};
use super::{mapping::DisplayMetrics, DcObject, DC_TYPE_DISPLAY, DC_TYPE_INFORMATION};
use crate::arch::common::syscall_frame::SyscallFrame;

pub(super) struct DetachedSurface {
    surface: u64,
}

impl DcObject {
    pub fn is_information_dc(&self) -> bool {
        self.information.is_some()
    }

    /// Resolve a drawing binding once. A detached information DC must never
    /// fall back to the primary framebuffer when its surface field is zero.
    pub(crate) fn drawing_surface(&self) -> *mut GdiSurface {
        if self.is_information_dc() {
            core::ptr::null_mut()
        } else if self.surface != 0 {
            self.surface as *mut GdiSurface
        } else {
            super::super::surface::get_primary_surface()
        }
    }

    fn make_information(&mut self, enable: bool) -> bool {
        if enable {
            // The original accepts normal device DCs, not memory/metafile
            // contexts or an information DC already in the requested state.
            // This repository currently realizes only display device DCs.
            if self.dc_type != DC_TYPE_DISPLAY || self.information.is_some() {
                return false;
            }
            let detached = DetachedSurface {
                surface: self.surface,
            };
            let device = DisplayMetrics::current().pixels;
            self.surface = 0;
            self.dc_type = DC_TYPE_INFORMATION;
            self.bounds = Rect::new(0, 0, device.cx, device.cy);
            self.information = Some(detached);
        } else {
            if self.dc_type != DC_TYPE_INFORMATION || self.information.is_none() {
                return false;
            }
            let detached = self.information.take().expect("checked detached DC");
            self.surface = detached.surface;
            self.dc_type = DC_TYPE_DISPLAY;
            if self.surface != 0 {
                // SAFETY: detached display bindings retain their existing
                // device-owned lifetime, just like attached display DCs.
                let surface = unsafe { &*(self.surface as *const GdiSurface) };
                self.bounds = Rect::new(0, 0, surface.width, surface.height);
            }
        }
        true
    }
}

pub fn make_information_dc(handle: u64, enable: bool) -> bool {
    super::get_dc(handle).is_some_and(|mut dc| dc.make_information(enable))
}

pub extern "C" fn nt_gdi_make_info_dc(frame: &SyscallFrame) -> u64 {
    u64::from(make_information_dc(
        frame.args[0],
        frame.args[1] as u32 != 0,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::super::{clipping, raster, region_geometry::Region};
    use super::*;

    #[test]
    fn information_dc_detaches_real_pixels_and_restores_the_borrowed_binding() {
        let mut surface = GdiSurface::new(8, 6, 32).unwrap();
        let mut dc = DcObject::new();
        dc.surface = (&mut surface as *mut GdiSurface) as u64;
        dc.bounds = Rect::new(0, 0, 8, 6);
        assert!(raster::set_pixel(&dc, 2, 3, 0x112233));
        let original = surface.get_pixel(2, 3);
        assert!(dc.make_information(true));
        assert_eq!(dc.surface, 0);
        assert!(dc.drawing_surface().is_null());
        assert!(!raster::set_pixel(&dc, 2, 3, 0xaabbcc));
        assert_eq!(raster::get_pixel(&dc, 2, 3), u32::MAX);
        assert!(!raster::paint_region(
            &dc,
            &Region::rectangle(Rect::new(0, 0, 8, 6)).unwrap(),
            0xaabbcc,
            0x00f00021
        ));
        assert_eq!(surface.get_pixel(2, 3), original);
        assert!(clipping::visible_region(&dc).unwrap().contains_point(2, 3));
        assert!(!dc.make_information(true));
        assert!(dc.make_information(false));
        assert_eq!((dc.bounds.right, dc.bounds.bottom), (8, 6));
        assert!(raster::set_pixel(&dc, 2, 3, 0xaabbcc));
        assert_eq!(raster::get_pixel(&dc, 2, 3), 0xaabbcc);
        assert!(!dc.make_information(false));
    }

    #[test]
    fn information_dc_without_a_surface_keeps_the_device_size_when_restored() {
        let mut dc = DcObject::new();
        dc.bounds = Rect::new(0, 0, 13, 17);
        assert!(dc.make_information(true));
        let size = dc.bounds;
        assert!(dc.make_information(false));
        assert_eq!(dc.bounds, size);
        assert_eq!(dc.surface, 0);
    }

    #[test]
    fn information_dc_source_and_legacy_renderers_do_not_recover_the_surface() {
        let mut source_surface = GdiSurface::new(8, 8, 32).unwrap();
        let mut destination_surface = GdiSurface::new(8, 8, 32).unwrap();
        let mut source = DcObject::new();
        source.surface = (&mut source_surface as *mut GdiSurface) as u64;
        let mut destination = DcObject::new();
        destination.surface = (&mut destination_surface as *mut GdiSurface) as u64;
        assert!(source.make_information(true));
        assert!(!raster::bit_blt(
            &destination,
            0,
            0,
            4,
            4,
            Some(&source),
            0,
            0,
            0x00cc0020
        ));
        // BLACKNESS does not consume the source and remains a real draw.
        assert!(raster::bit_blt(
            &destination,
            0,
            0,
            4,
            4,
            Some(&source),
            0,
            0,
            0x00000042
        ));
        assert!(!super::super::super::gdi_ops::gre_ellipse(
            &mut source,
            0,
            0,
            4,
            4
        ));
        assert_eq!(destination_surface.get_pixel(1, 1), Some(0));
    }

    #[test]
    fn information_dc_entry_uses_the_native_bool_width_and_valid_handle() {
        let handle = super::super::get_dc_table().create(DC_TYPE_DISPLAY, 0);
        assert_ne!(handle, 0);
        let invoke = |dc, direction| {
            let mut frame = SyscallFrame::default();
            frame.args[0] = dc;
            frame.args[1] = direction;
            nt_gdi_make_info_dc(&frame)
        };
        let call = |direction| invoke(handle, direction);
        assert_eq!(call(0x1_0000_0000), 0);
        assert_eq!(call(0x1_0000_0001), 1);
        assert_eq!(call(u64::MAX), 0);
        assert_eq!(call(0x1_0000_0000), 1);
        assert_eq!(call(0xffff_ffff), 1);
        assert_eq!(call(0), 1);
        assert!(super::super::gre_delete_dc(handle));
        assert_eq!(call(1), 0);
        assert_eq!(invoke(0, 1), 0);
    }

    #[test]
    fn information_dc_cannot_select_a_bitmap_or_change_its_device_size() {
        let handle = super::super::get_dc_table().create(DC_TYPE_DISPLAY, 0);
        let bitmap = super::super::super::objects::gdi_create_bitmap(3, 2, 32);
        assert_ne!(bitmap, 0);
        assert!(make_information_dc(handle, true));
        let before = super::super::get_dc(handle).unwrap().bounds;
        assert_eq!(super::super::gre_select_object(handle, bitmap), 0);
        let after = super::super::get_dc(handle).unwrap().bounds;
        assert_eq!(after, before);
        assert!(super::super::gre_delete_dc(handle));
        assert!(super::super::super::objects::gdi_delete_object(bitmap));
    }

    #[test]
    fn information_dc_is_not_a_saved_attribute_or_a_memory_dc_conversion() {
        let mut dc = DcObject::new();
        assert!(dc.save_state());
        assert!(dc.make_information(true));
        assert!(dc.restore_state());
        assert!(dc.is_information_dc());
        assert!(dc.drawing_surface().is_null());
        assert!(dc.save_state());
        assert!(dc.make_information(false));
        assert!(dc.restore_state());
        assert!(!dc.is_information_dc());
        for kind in [
            super::super::DC_TYPE_MEMORY,
            super::super::DC_TYPE_METAFILE,
            super::super::DC_TYPE_PRINTER,
        ] {
            dc.dc_type = kind;
            assert!(!dc.make_information(true));
            assert!(!dc.make_information(false));
            assert_eq!(dc.dc_type, kind);
        }
    }

    #[test]
    fn deleting_an_information_dc_leaves_its_device_surface_alive() {
        let mut surface = GdiSurface::new(3, 2, 32).unwrap();
        let handle = super::super::get_dc_table()
            .create(DC_TYPE_DISPLAY, (&mut surface as *mut GdiSurface) as u64);
        assert_ne!(handle, 0);
        assert!(make_information_dc(handle, true));
        assert!(super::super::gre_delete_dc(handle));
        assert!(!make_information_dc(handle, false));
        assert!(surface.set_pixel(1, 1, 0x123456));
        assert_eq!(surface.get_pixel(1, 1), Some(0x123456));
    }
}
