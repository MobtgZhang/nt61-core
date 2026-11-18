//! Cache only the glass rim, sampled before painting the window itself.
//! A hover reuses these strips; changes behind the glass rebuild them.

use alloc::vec::Vec;

use super::{colorize_glass, GLASS_BLUR_RADIUS, GLASS_SATURATION, GLASS_TINT};
use crate::graphics::{Rect, Surface};

#[derive(Default)]
pub struct WindowGlassCache {
    windows: Vec<WindowGlass>,
}

struct WindowGlass {
    id: u32,
    bounds: Rect,
    extension: u32,
    footer: u32,
    strips: Vec<GlassStrip>,
}

struct GlassStrip {
    bounds: Rect,
    pixels: Surface,
}

impl WindowGlassCache {
    pub fn retain(&mut self, visible: impl Fn(u32) -> bool) {
        self.windows.retain(|window| visible(window.id));
    }

    /// Damage adjacent to a glass strip also affects its blur kernel. Rebuild
    /// the lower layers in full so the kernel never samples stale upper layers.
    pub fn intersects_blur(&self, damage: Rect, above: impl Fn(u32) -> bool) -> bool {
        self.windows
            .iter()
            .filter(|window| above(window.id))
            .any(|window| {
                window.strips.iter().any(|strip| {
                    inflate(strip.bounds, GLASS_BLUR_RADIUS)
                        .intersection(&damage)
                        .is_some()
                })
            })
    }

    pub fn prepare(
        &mut self,
        backdrop: &Surface,
        id: u32,
        bounds: Rect,
        extension: u32,
        refresh: bool,
    ) {
        self.prepare_extended(backdrop,id,bounds,extension,0,refresh);
    }

    /// Extend DWM glass into the Photo Viewer transport strip as well as the caption.
    pub fn prepare_extended(&mut self,backdrop:&Surface,id:u32,bounds:Rect,extension:u32,footer:u32,refresh:bool) {
        let index = self.windows.iter().position(|window| window.id == id);
        if let Some(index) = index {
            let window = &self.windows[index];
            if !refresh && window.bounds == bounds && window.extension == extension && window.footer==footer {
                return;
            }
        }
        let rim = 6.min(bounds.width / 2).min(bounds.height / 2);
        let top = (30 + extension).min(bounds.height.saturating_sub(rim));
        let bottom=(rim+footer).min(bounds.height.saturating_sub(top));
        let side_height = bounds.height.saturating_sub(top + bottom);
        let strips = [
            Rect::new(bounds.x, bounds.y, bounds.width, top),
            Rect::new(bounds.x, bounds.y + top as i32, rim, side_height),
            Rect::new(
                bounds.x + (bounds.width - rim) as i32,
                bounds.y + top as i32,
                rim,
                side_height,
            ),
            Rect::new(
                bounds.x,
                bounds.y + (bounds.height - bottom) as i32,
                bounds.width,
                bottom,
            ),
        ]
        .into_iter()
        .filter_map(|strip| GlassStrip::capture(backdrop, strip))
        .collect();
        let window = WindowGlass {
            id,
            bounds,
            extension,
            footer,
            strips,
        };
        if let Some(index) = index {
            self.windows[index] = window;
        } else {
            self.windows.push(window);
        }
    }

    pub fn paint(&self, surface: &mut Surface, id: u32, active: bool) {
        let Some(window) = self.windows.iter().find(|window| window.id == id) else {
            return;
        };
        for strip in &window.strips {
            strip.paint(surface, window.bounds, active);
        }
    }
}

impl GlassStrip {
    fn capture(backdrop: &Surface, bounds: Rect) -> Option<Self> {
        let screen = Rect::new(0, 0, backdrop.width(), backdrop.height());
        let visible = bounds.intersection(&screen)?;
        // Include the blur halo, then crop it. Omitting it creates seams where
        // the caption, sides and bottom meet, or where a window moves on screen.
        let sample = inflate(visible, GLASS_BLUR_RADIUS).intersection(&screen)?;
        let mut pixels = Surface::new(sample.width, sample.height);
        pixels.copy_region(backdrop, sample, 0, 0);
        pixels.box_blur(GLASS_BLUR_RADIUS);
        colorize_glass(&mut pixels, GLASS_TINT, GLASS_SATURATION);
        // DWM glass has a neutral luminosity component as well as colorization.
        // Without it, the default blue wallpaper produces a dark solid caption.
        let mut cropped = Surface::new(visible.width, visible.height);
        cropped.copy_region(
            &pixels,
            Rect::new(
                visible.x - sample.x,
                visible.y - sample.y,
                visible.width,
                visible.height,
            ),
            0,
            0,
        );
        Some(Self {
            bounds: visible,
            pixels: cropped,
        })
    }

    fn paint(&self, surface: &mut Surface, window: Rect, active: bool) {
        let Some(clip) = self.bounds.intersection(&surface.drawing_bounds()) else {
            return;
        };
        for y in clip.y..clip.y + clip.height as i32 {
            let local_y = (y - window.y) as u32;
            // The glass itself follows the rounded silhouette, not just its
            // outline. Preserve the lower scene in the four corner cutouts.
            let edge = local_y.min(window.height.saturating_sub(local_y + 1));
            let inset = match edge {
                0 => 4,
                1 => 2,
                2 | 3 => 1,
                _ => 0,
            };
            let left = clip.x.max(window.x + inset);
            let right = (clip.x + clip.width as i32).min(window.x + window.width as i32 - inset);
            if left >= right {
                continue;
            }
            let row = Rect::new(left, y, (right - left) as u32, 1);
            surface.copy_region(
                &self.pixels,
                Rect::new(left - self.bounds.x, y - self.bounds.y, row.width, 1),
                left,
                y,
            );
            if !active {
                surface.blend_rect(row, 0x48E7EDF2);
            }
            // Broad diagonal reflections cross both caption and side glass.
            // They are translucent so underlying windows remain visible.
            for x in left..right {
                let diagonal = (x - window.x) + (y - window.y) * 2;
                let band = diagonal.rem_euclid(480);
                let reflection = if band < 90 { (90 - band) * 38 / 90 } else { 0 };
                let caption_glow = if local_y < 18 {
                    (18 - local_y) * 24 / 18
                } else {
                    0
                };
                let alpha = reflection as u32 + caption_glow;
                if alpha != 0 {
                    surface.blend_pixel(x as u32, y as u32, (alpha << 24) | 0x00FFFFFF);
                }
            }
        }
    }
}

fn inflate(rect: Rect, radius: u32) -> Rect {
    Rect::new(
        rect.x.saturating_sub(radius as i32),
        rect.y.saturating_sub(radius as i32),
        rect.width.saturating_add(radius * 2),
        rect.height.saturating_add(radius * 2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_footer_samples_the_backdrop_and_invalidates_its_cache() {
        let mut backdrop=Surface::new(120,120);backdrop.fill_rect(0,0,120,120,0xff507ca1);
        let bounds=Rect::new(10,10,100,100);let mut cache=WindowGlassCache::default();
        cache.prepare(&backdrop,1,bounds,0,true);
        let mut frame=backdrop.clone();cache.paint(&mut frame,1,true);
        assert_eq!(frame.pixel(50,85),backdrop.pixel(50,85));
        cache.prepare_extended(&backdrop,1,bounds,0,32,false);
        frame.copy_from(&backdrop);cache.paint(&mut frame,1,true);
        assert_ne!(frame.pixel(50,85),backdrop.pixel(50,85));
        assert_eq!(frame.pixel(50,60),backdrop.pixel(50,60));
        assert!(cache.intersects_blur(Rect::new(50,85,1,1),|_|true));
        cache.prepare_extended(&backdrop,1,bounds,0,0,false);
        frame.copy_from(&backdrop);cache.paint(&mut frame,1,true);
        assert_eq!(frame.pixel(50,85),backdrop.pixel(50,85));
    }
    #[test]
    fn clipped_hover_replay_matches_full_composition_at_every_screen_edge() {
        for bounds in [Rect::new(-20, -10, 100, 80), Rect::new(40, 30, 100, 80)] {
            let mut backdrop = Surface::new(120, 100);
            for y in 0..100 {
                backdrop.fill_rect(0, y, 120, 1, 0xFF203040 + y as u32 * 0x010101);
            }
            let mut cache = WindowGlassCache::default();
            cache.prepare(&backdrop, 1, bounds, 36, true);
            let mut full = backdrop.clone();
            cache.paint(&mut full, 1, true);
            let damage = Rect::new(15, 0, 70, 100);
            let mut partial = backdrop.clone();
            partial.set_draw_clip(Some(damage));
            cache.paint(&mut partial, 1, true);
            for y in 0..100 {
                for x in 0..120 {
                    let expected = if damage.contains(x, y) {
                        full.pixel(x as u32, y as u32)
                    } else {
                        backdrop.pixel(x as u32, y as u32)
                    };
                    assert_eq!(partial.pixel(x as u32, y as u32), expected);
                }
            }
        }
    }

    #[test]
    fn cached_glass_blurs_detail_preserves_corners_and_reuses_lower_scene() {
        let mut lower = Surface::new(120, 100);
        lower.fill_rect(0, 0, 120, 100, 0xFF202020);
        lower.fill_rect(60, 0, 60, 100, 0xFFE0E0E0);
        let bounds = Rect::new(10, 10, 100, 80);
        let mut cache = WindowGlassCache::default();
        cache.prepare(&lower, 7, bounds, 0, true);
        let mut frame = lower.clone();
        cache.paint(&mut frame, 7, true);
        assert_eq!(frame.pixel(10, 10), lower.pixel(10, 10));
        let left = frame.pixel(59, 28).unwrap() & 255;
        let right = frame.pixel(60, 28).unwrap() & 255;
        assert!(
            right > left && right - left < 16,
            "blur must soften the sharp edge"
        );
        let oracle = frame.clone();
        // Hover composition starts from the lower scene, never from the last
        // tinted frame. Repainting must not accumulate tint or lose transparency.
        for _ in 0..3 {
            frame.copy_from(&lower);
            cache.prepare(&frame, 7, bounds, 0, false);
            cache.paint(&mut frame, 7, true);
            assert_eq!(frame, oracle);
        }
        assert!(cache.intersects_blur(Rect::new(60, 42, 2, 2), |_| true));
        assert!(!cache.intersects_blur(Rect::new(50, 55, 2, 2), |_| true));
        assert!(
            !cache.intersects_blur(Rect::new(60, 20, 2, 2), |id| id > 7),
            "a foreground change cannot invalidate glass below it"
        );
        frame.copy_from(&lower);
        cache.paint(&mut frame, 7, false);
        assert_ne!(frame.pixel(30, 25), oracle.pixel(30, 25));
    }
}
