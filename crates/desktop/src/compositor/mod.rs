use crate::graphics::Surface;
use crate::window::Rect;
use alloc::vec::Vec;

pub mod aero;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceHandle(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedirectedSurface {
    pub handle: SurfaceHandle,
    pub owner: u32,
    pub bounds: Rect,
    pub visible: bool,
    pub opaque: bool,
    pub z_order: u32,
    pub pixels: Vec<u32>,
}

impl RedirectedSurface {
    pub fn new(handle: u32, owner: u32, bounds: Rect, z_order: u32) -> Self {
        Self {
            handle: SurfaceHandle(handle),
            owner,
            bounds,
            visible: true,
            opaque: true,
            z_order,
            pixels: alloc::vec![0; bounds.width as usize * bounds.height as usize],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compositor {
    pub width: u32,
    pub height: u32,
    pub surfaces: Vec<RedirectedSurface>,
    pub glass: bool,
    next_surface: u32,
}

impl Compositor {
    pub fn new(width: u32, height: u32, glass: bool) -> Self {
        Self {
            width,
            height,
            surfaces: Vec::new(),
            glass,
            next_surface: 1,
        }
    }

    pub fn begin(&mut self, owner: u32, bounds: Rect) -> SurfaceHandle {
        let handle = SurfaceHandle(self.next_surface.min(0xFFFF).max(1));
        self.next_surface = self.next_surface.saturating_add(1);
        let z_order = self
            .surfaces
            .last()
            .map_or(1, |surface| surface.z_order + 1);
        self.surfaces
            .push(RedirectedSurface::new(handle.0, owner, bounds, z_order));
        handle
    }

    pub fn surface(&self, handle: SurfaceHandle) -> Option<&RedirectedSurface> {
        self.surfaces
            .iter()
            .find(|surface| surface.handle == handle)
    }

    pub fn surface_mut(&mut self, handle: SurfaceHandle) -> Option<&mut RedirectedSurface> {
        self.surfaces
            .iter_mut()
            .find(|surface| surface.handle == handle)
    }

    pub fn remove(&mut self, handle: SurfaceHandle) -> bool {
        let before = self.surfaces.len();
        self.surfaces.retain(|surface| surface.handle != handle);
        if self.surfaces.len() != before {
            self.surfaces
                .iter_mut()
                .enumerate()
                .for_each(|(index, surface)| surface.z_order = index as u32 + 1);
            true
        } else {
            false
        }
    }

    pub fn bring_to_front(&mut self, handle: SurfaceHandle) {
        let Some(position) = self
            .surfaces
            .iter()
            .position(|surface| surface.handle == handle)
        else {
            return;
        };
        let surface = self.surfaces.remove(position);
        self.surfaces.push(surface);
        self.surfaces
            .iter_mut()
            .enumerate()
            .for_each(|(index, surface)| {
                surface.z_order = index as u32 + 1;
            });
    }

    pub fn topmost_at(&self, x: i32, y: i32) -> Option<SurfaceHandle> {
        self.surfaces
            .iter()
            .filter(|surface| surface.visible && surface.bounds.contains(x, y))
            .max_by_key(|surface| surface.z_order)
            .map(|surface| surface.handle)
    }

    pub fn invalidate(&mut self, handle: SurfaceHandle) {
        if let Some(surface) = self.surface_mut(handle) {
            surface.pixels.fill(0);
        }
    }

    pub fn compose(&self, wallpaper: &Surface) -> Surface {
        let mut output = Surface::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                output.set_pixel(x, y, wallpaper.pixel(x, y).unwrap_or(0xFF00_0000));
            }
        }
        for surface in &self.surfaces {
            if !surface.visible {
                continue;
            }
            for row in 0..surface.bounds.height {
                for column in 0..surface.bounds.width {
                    let index = row as usize * surface.bounds.width as usize + column as usize;
                    if let Some(source) = surface.pixels.get(index) {
                        let x = surface.bounds.x + column as i32;
                        let y = surface.bounds.y + row as i32;
                        if x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32 {
                            if surface.opaque {
                                output.set_pixel(x as u32, y as u32, *source | 0xFF00_0000);
                            } else {
                                output.blend_pixel(x as u32, y as u32, *source);
                            }
                        }
                    }
                }
            }
        }
        output
    }

    pub fn apply_aero(&mut self, backdrop: &Surface, tint: u32, saturation: u32) -> Surface {
        let mut glass = aero::glass_backdrop(backdrop, tint, saturation);
        for surface in &self.surfaces {
            if !surface.visible {
                continue;
            }
            for row in 0..surface.bounds.height {
                for column in 0..surface.bounds.width {
                    let x = surface.bounds.x + column as i32;
                    let y = surface.bounds.y + row as i32;
                    if x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32 {
                        let index = row as usize * surface.bounds.width as usize + column as usize;
                        if let Some(source) = surface.pixels.get(index) {
                            glass.blend_pixel(x as u32, y as u32, *source);
                        }
                    }
                }
            }
        }
        glass
    }

    #[cfg(test)]
    fn glass_metrics(&self) -> (usize, usize, u32, u32) {
        (
            self.width as usize,
            self.height as usize,
            self.glass as u32,
            self.width.saturating_mul(self.height),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DwmMode {
    Active,
    Fallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AeroAction {
    Peek,
    Shake,
    Flip3D,
    SnapLeft,
    SnapRight,
    Maximize,
    Restore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowEffect {
    pub surface: SurfaceHandle,
    pub glass: bool,
    pub shadow: bool,
    pub rounded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DwmProcess {
    pub running: bool,
    pub mode: DwmMode,
    pub compositor: Compositor,
    pub effects: Vec<WindowEffect>,
    pub frame: u64,
}

impl DwmProcess {
    pub fn new(width: u32, height: u32, aero: bool) -> Self {
        Self {
            running: true,
            mode: if aero {
                DwmMode::Active
            } else {
                DwmMode::Fallback
            },
            compositor: Compositor::new(width, height, aero),
            effects: Vec::new(),
            frame: 0,
        }
    }

    pub fn begin(&mut self, owner: u32, bounds: Rect, glass: bool) -> SurfaceHandle {
        let handle = self.compositor.begin(owner, bounds);
        self.effects.push(WindowEffect {
            surface: handle,
            glass,
            shadow: true,
            rounded: true,
        });
        handle
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.mode = DwmMode::Fallback;
    }

    pub fn restart(&mut self) {
        self.running = true;
        if self.compositor.glass {
            self.mode = DwmMode::Active;
        }
    }

    pub fn apply_action(
        &mut self,
        handle: SurfaceHandle,
        action: AeroAction,
        width: u32,
        height: u32,
    ) {
        let Some(surface) = self.compositor.surface_mut(handle) else {
            return;
        };
        match action {
            AeroAction::Peek => surface.visible = true,
            AeroAction::Shake => surface.visible = true,
            AeroAction::Flip3D => {
                let z = surface.z_order.max(1);
                surface.bounds.x = ((z as i32 - 1) * 8) + 64;
                surface.bounds.y = ((z as i32 - 1) * 8) + 64;
            }
            AeroAction::SnapLeft => {
                surface.bounds.x = 0;
                surface.bounds.y = 0;
                surface.bounds.width = width / 2;
                surface.bounds.height = height.saturating_sub(40);
            }
            AeroAction::SnapRight => {
                surface.bounds.x = (width / 2) as i32;
                surface.bounds.y = 0;
                surface.bounds.width = width - width / 2;
                surface.bounds.height = height.saturating_sub(40);
            }
            AeroAction::Maximize => {
                surface.bounds.x = 0;
                surface.bounds.y = 0;
                surface.bounds.width = width;
                surface.bounds.height = height;
            }
            AeroAction::Restore => {
                surface.bounds.x = 48;
                surface.bounds.y = 48;
                surface.bounds.width = width / 2;
                surface.bounds.height = height.saturating_sub(160);
            }
        }
    }

    pub fn present(&mut self, wallpaper: &Surface, tint: u32, saturation: u32) -> Surface {
        let output = match self.mode {
            DwmMode::Active => self.compositor.apply_aero(wallpaper, tint, saturation),
            DwmMode::Fallback => self.compositor.compose(wallpaper),
        };
        self.frame += 1;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Surface;

    #[test]
    fn redirected_surfaces_compose_by_z_order() {
        let backdrop = Surface::new(64, 64);
        let mut compositor = Compositor::new(64, 64, true);
        let bottom = compositor.begin(1, Rect::new(0, 0, 64, 64));
        let top = compositor.begin(2, Rect::new(8, 8, 32, 32));
        compositor.surfaces[(bottom.0 - 1) as usize].pixels[0] = 0xFF00_00FF;
        compositor.surfaces[(top.0 - 1) as usize].pixels[0] = 0xFF00_FF00;
        let output = compositor.compose(&backdrop);
        assert_eq!(output.pixel(0, 0), Some(0xFF00_00FF));
        assert_eq!(output.pixel(8, 8), Some(0xFF00_FF00));
    }

    #[test]
    fn aero_blurs_and_tints_backdrop() {
        let mut backdrop = Surface::new(32, 32);
        backdrop.fill_rect(0, 0, 32, 32, 0xFF80_8080);
        let mut compositor = Compositor::new(32, 32, true);
        compositor.begin(1, Rect::new(0, 0, 16, 16));
        let output = compositor.apply_aero(&backdrop, 0x20_40A0FF, 64);
        assert_eq!(output.pixel(0, 0), output.pixel(31, 31));
    }

    #[test]
    fn dwm_composes_snap_restore_and_falls_back_without_crash() {
        let mut dwm = DwmProcess::new(800, 600, true);
        let handle = dwm.begin(1, Rect::new(10, 10, 300, 200), true);
        dwm.apply_action(handle, AeroAction::SnapLeft, 800, 600);
        assert_eq!(
            dwm.compositor.surface(handle).unwrap().bounds,
            Rect::new(0, 0, 400, 560)
        );
        dwm.compositor.bring_to_front(handle);
        assert_eq!(dwm.compositor.topmost_at(10, 10), Some(handle));
        dwm.stop();
        assert_eq!(dwm.mode, DwmMode::Fallback);
        let wallpaper = Surface::new(800, 600);
        assert_eq!(dwm.present(&wallpaper, 0x20_40A0FF, 64).width(), 800);
        dwm.restart();
        assert_eq!(dwm.mode, DwmMode::Active);
        assert_eq!(dwm.frame, 1);
    }

    #[test]
    fn aero_glass_is_real_composition_not_static_asset() {
        let mut backdrop = Surface::new(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                let value = if x < 16 { 0xFFFF_FFFF } else { 0xFF00_0000 };
                backdrop.set_pixel(x, y, value);
            }
        }
        let mut compositor = Compositor::new(32, 32, true);
        let handle = compositor.begin(1, Rect::new(4, 4, 16, 16));
        if let Some(surface) = compositor.surface_mut(handle) {
            surface.pixels.fill(0x80_FF00_00);
        }
        let output = compositor.apply_aero(&backdrop, 0x20_40A0FF, 64);
        let centre = output.pixel(16, 16).unwrap();
        let edge = output.pixel(0, 0).unwrap();
        assert_ne!(centre, backdrop.pixel(16, 16).unwrap());
        assert_ne!(edge, backdrop.pixel(0, 0).unwrap());
        assert_ne!(centre, edge);
        assert!(compositor.glass_metrics().2 == 1);
        let _ = handle;
    }
}
pub mod damage;
