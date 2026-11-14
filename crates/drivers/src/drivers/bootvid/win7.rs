//! Windows 7 "Starting Windows" boot animation.
//!
//! This is a compact integer/fixed-point port of the offline preview
//! renderer: four colored orbs converge, a central flash blooms, and
//! the four-pane Windows flag unfolds with soft breathing rays.

use core::f32::consts::PI;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::drivers::virtio::gpu::GpuSession;
use crate::drivers::virtio::pci::PciTransport;
use crate::ke::sync::Spinlock;

const T_TEXT0: u32 = 120;
const T_TEXT1: u32 = 620;
const T_CPY0: u32 = 200;
const T_CPY1: u32 = 700;
const T_ORB0: u32 = 550;
const T_ENTRY1: u32 = 2400;
const T_DANCE1: u32 = 3600;
const T_MERGE1: u32 = 3850;
const T_FLASH0: u32 = 3700;
const T_FLASH1: u32 = 4500;
const T_BLOOM0: u32 = 3700;
const T_BLOOM1: u32 = 4500;
const BREATH_PERIOD_MS: u32 = 3000;
const WIN7_ANIMATION_MS: u32 = 7200;

const FLAG_FW: f32 = 64.0;
const FLAG_FH: f32 = 62.0;
const FLAG_GAP: f32 = 3.0;

const ORB_RGB: [[f32; 3]; 4] = [
    [242.0, 80.0, 34.0],
    [127.0, 186.0, 0.0],
    [0.0, 164.0, 239.0],
    [255.0, 185.0, 0.0],
];

const ORB_ENTRY: [[f32; 2]; 4] = [[-0.14, -0.12], [1.14, -0.12], [-0.14, 1.12], [1.14, 1.12]];

struct AnimationCanvas {
    ptr: *mut u8,
    len: usize,
}

unsafe impl Send for AnimationCanvas {}

impl AnimationCanvas {
    const fn new() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            len: 0,
        }
    }
}

static ANIMATION_CANVAS: Spinlock<AnimationCanvas> = Spinlock::new(AnimationCanvas::new());
// A GPU resource is not a CPU-visible firmware framebuffer. Keep the
// desktop's backing store separate from the resource used for scanout.
static SOFTWARE_FRAMEBUFFER: Spinlock<AnimationCanvas> = Spinlock::new(AnimationCanvas::new());
static BOOT_GPU: Spinlock<Option<GpuSession>> = Spinlock::new(None);
static BOOT_GPU_UNAVAILABLE: AtomicBool = AtomicBool::new(false);
static DEBUG_FRAME: AtomicU32 = AtomicU32::new(0);
static GPU_PROBE_DEBUG: AtomicBool = AtomicBool::new(false);
static GPU_PRESENT_FAILED: AtomicBool = AtomicBool::new(false);

fn gpu_present(width: u32, height: u32, canvas_ptr: *mut u8) -> bool {
    if !GPU_PROBE_DEBUG.load(Ordering::Acquire) {
        GPU_PROBE_DEBUG.store(true, Ordering::Release);
        crate::hal::serial::write_string_always("[ANIM] gpu present attempt\r\n");
    }
    if canvas_ptr.is_null() || BOOT_GPU_UNAVAILABLE.load(Ordering::Acquire) {
        return false;
    }

    let mut session_guard = BOOT_GPU.lock();
    if session_guard.is_none() {
        let mut transport = None;
        for bus in 0..2u8 {
            for device in 0..32u8 {
                let vendor = crate::hal::common::pci::read_config_word(bus, device, 0, 0);
                let device_id = crate::hal::common::pci::read_config_word(bus, device, 0, 2);
                if vendor != 0x1af4 || device_id != 0x1050 {
                    continue;
                }
                let Some(candidate) = (unsafe { PciTransport::probe(0, bus, device, 0) }) else {
                    continue;
                };
                crate::hal::serial::write_string_always("[ANIM] gpu transport probe ok\r\n");
                transport = Some(candidate);
                break;
            }
            if transport.is_some() {
                break;
            }
        }

        let Some(mut transport) = transport else {
            // PCI mappings can become available after the first boot frame.
            // A failed transport probe must not disable the later handover.
            return false;
        };
        // Allocate the CPU framebuffer before handing any backing memory to
        // the device. Allocation failure can then be retried without leaving
        // an active DMA resource owned by a discarded GPU session.
        let software_framebuffer = if !super::is_lfb_active() {
            let mut framebuffer = SOFTWARE_FRAMEBUFFER.lock();
            let Some((pointer, _)) = ensure_canvas(&mut framebuffer, width, height) else {
                return false;
            };
            Some(pointer)
        } else {
            None
        };
        let Some(mut gpu) =
            (unsafe { crate::drivers::virtio::gpu::VirtioGpu::probe(0, &mut transport) })
        else {
            crate::hal::serial::write_string_always("[ANIM] gpu device probe failed\r\n");
            BOOT_GPU_UNAVAILABLE.store(true, Ordering::Release);
            return false;
        };
        let initialized =
            unsafe { gpu.initialize_display(&mut transport, 0, canvas_ptr, width, height) };
        if !initialized {
            crate::hal::serial::write_string_always("[ANIM] gpu display init failed\r\n");
            BOOT_GPU_UNAVAILABLE.store(true, Ordering::Release);
            return false;
        }
        crate::hal::serial::write_string_always("[ANIM] gpu display init ok\r\n");
        if let Some(pointer) = software_framebuffer {
            let info = crate::hal::common::framebuffer_impl::FramebufferInfo {
                address: pointer as u64,
                width,
                height,
                pitch: width * 4,
                bpp: 32,
            };
            crate::hal::common::framebuffer_impl::init(info);
            #[cfg(target_arch = "x86_64")]
            crate::hal::framebuffer::init(Some(crate::hal::framebuffer::FramebufferInfo {
                address: info.address, width, height, pitch: info.pitch, bpp: 32,
            }));
            super::init_from_framebuffer(info.address, width, height, info.pitch);
            crate::hal::serial::write_string_always("[ANIM] installed GPU software framebuffer\r\n");
        }
        *session_guard = Some(GpuSession { gpu, transport });
        return true;
    }

    let result = session_guard
        .as_mut()
        .map(|session| session.gpu.present_display(&mut session.transport))
        .unwrap_or(false);
    if result && DEBUG_FRAME.load(Ordering::Acquire) % 60 == 0 {
        crate::hal::serial::write_string_always("[ANIM] gpu present done\r\n");
    }
    result
}

pub(super) fn paint_boot_screen(percent: u32) {
    paint_boot_screen_time(percent.clamp(0, 100) * WIN7_ANIMATION_MS / 100);
}

pub(super) fn paint_boot_screen_time(t_ms: u32) {
    let _debug_frame = DEBUG_FRAME.fetch_add(1, Ordering::AcqRel);
    let has_lfb = super::is_lfb_active() && super::fb_base() != 0;
    let (width, height, pitch) = if has_lfb {
        (super::fb_width(), super::fb_height(), super::fb_pitch())
    } else {
        const WIDTH: u32 = 1024;
        const HEIGHT: u32 = 768;
        (WIDTH, HEIGHT, WIDTH * 4)
    };
    if width == 0 || height == 0 || pitch == 0 {
        return;
    }

    let mut canvas_guard = ANIMATION_CANVAS.lock();
    let Some((canvas_ptr, canvas_len)) = ensure_canvas(&mut canvas_guard, width, height) else {
        return;
    };
    unsafe {
        core::ptr::write_bytes(canvas_ptr, 0, canvas_len);
    }
    super::set_render_target(canvas_ptr as u64, width * 4);

    let scale = width.min(height) as f32 / 768.0;
    let cx = width as f32 * 0.5;
    let cy = height as f32 * 0.5;
    prepare_ray_field(width, height, cx, cy, scale);
    // Render and present on the display owner. An AP must never keep writing
    // a strip after this frame is presented or the next frame reuses its buffer.
    let mut frame = FrameView::new(canvas_ptr, width, height);
    draw_rays(&mut frame, cx, cy, scale, t_ms);
    draw_flag(&mut frame, cx, cy, scale, t_ms);
    draw_orbs(&mut frame, cx, cy, scale, t_ms);
    draw_flash(&mut frame, cx, cy, scale, t_ms);
    draw_text(width, height, cx, cy, scale, t_ms);

    if !gpu_present(width, height, canvas_ptr) {
        if !GPU_PRESENT_FAILED.swap(true, Ordering::AcqRel) {
            crate::hal::serial::write_string_always("[ANIM] gpu present failed once\r\n");
        }
        copy_canvas_to_lfb(canvas_ptr, width, height, pitch);
    }

    super::clear_render_target();
}

pub fn poll_animation_strip(_cpu: u32) -> bool {
    false
}

/// Safe/debug modes still need a live scanout, even without the animation.
pub(super) fn initialize_basic_display() {
    if !super::is_lfb_active() || super::fb_base() == 0 {
        return;
    }
    let (width, height, pitch) = (super::fb_width(), super::fb_height(), super::fb_pitch());
    if width == 0 || height == 0 || pitch < width.saturating_mul(4) {
        return;
    }
    // Scanout and the mirror must share the same persistent, tightly packed
    // backing store. Firmware framebuffers may have padding between rows.
    let mut canvas = ANIMATION_CANVAS.lock();
    let Some((pointer, _)) = ensure_canvas(&mut canvas, width, height) else {
        return;
    };
    for row in 0..height as usize {
        unsafe {
            core::ptr::copy_nonoverlapping(
                (super::fb_base() as *const u8).add(row * pitch as usize),
                pointer.add(row * width as usize * 4),
                width as usize * 4,
            );
        }
    }
    let _ = gpu_present(width, height, pointer);
    crate::hal::common::framebuffer_impl::mark_damage(0, 0, width, height);
}

struct FrameView {
    base: *mut u32,
    width: u32,
    height: u32,
    y_start: u32,
    y_end: u32,
}

impl FrameView {
    fn new(base: *mut u8, width: u32, height: u32) -> Self {
        Self {
            base: base.cast::<u32>(),
            width,
            height,
            y_start: 0,
            y_end: height,
        }
    }



    #[inline]
    fn add(&mut self, x: u32, y: u32, blue: f32, green: f32, red: f32) {
        if x >= self.width || y >= self.height {
            return;
        }
        if y < self.y_start || y >= self.y_end {
            return;
        }
        let pixel = unsafe { self.base.add(((y * self.width) + x) as usize) };
        unsafe {
            let current = core::ptr::read(pixel);
            let alpha = current & 0xFF00_0000;
            let next_blue = ((current & 0xFF) as f32 + blue).min(255.0) as u32;
            let next_green = (((current >> 8) & 0xFF) as f32 + green).min(255.0) as u32;
            let next_red = (((current >> 16) & 0xFF) as f32 + red).min(255.0) as u32;
            core::ptr::write(
                pixel,
                alpha | next_blue | (next_green << 8) | (next_red << 16),
            );
        }
    }

    #[inline]
    fn set(&mut self, x: u32, y: u32, value: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        if y < self.y_start || y >= self.y_end {
            return;
        }
        unsafe {
            core::ptr::write(self.base.add(((y * self.width) + x) as usize), value);
        }
    }

    #[inline]
    fn blend(&mut self, x: u32, y: u32, blue: u32, green: u32, red: u32, alpha: u32) {
        if x >= self.width || y >= self.height || alpha == 0 {
            return;
        }
        if y < self.y_start || y >= self.y_end {
            return;
        }
        let inverse = 255 - alpha;
        let pixel = unsafe { self.base.add(((y * self.width) + x) as usize) };
        unsafe {
            let current = core::ptr::read(pixel);
            let next_blue = ((current & 0xFF) * inverse + blue * alpha + 127) / 255;
            let next_green = (((current >> 8) & 0xFF) * inverse + green * alpha + 127) / 255;
            let next_red = (((current >> 16) & 0xFF) * inverse + red * alpha + 127) / 255;
            core::ptr::write(
                pixel,
                0xFF00_0000 | next_blue | (next_green << 8) | (next_red << 16),
            );
        }
    }
}

fn ensure_canvas(
    canvas: &mut AnimationCanvas,
    width: u32,
    height: u32,
) -> Option<(*mut u8, usize)> {
    let canvas_len = width as usize * height as usize * 4;
    if canvas.ptr.is_null() || canvas.len != canvas_len {
        let canvas_phys = crate::mm::allocate_pages(canvas_len.div_ceil(4096) as u64)?;
        let map_start = canvas_phys & !0x1F_FFFF;
        let map_len =
            ((canvas_len as u64) + (canvas_phys - map_start)).div_ceil(0x20_0000) * 0x20_0000;
        if !crate::arch::identity_map_region(map_start, map_len) {
            return None;
        }
        unsafe {
            core::ptr::write_bytes(canvas_phys as *mut u8, 0, canvas_len);
        }
        canvas.ptr = canvas_phys as *mut u8;
        canvas.len = canvas_len;
    }
    Some((canvas.ptr, canvas.len))
}

fn copy_canvas_to_lfb(canvas: *mut u8, width: u32, height: u32, pitch: u32) {
    let fb_base = super::fb_base();
    if fb_base == 0 || pitch == 0 || width == 0 || height == 0 {
        return;
    }
    if pitch == width * 4 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                canvas,
                fb_base as *mut u8,
                width as usize * height as usize * 4,
            );
        }
        return;
    }
    for row in 0..height {
        unsafe {
            let source = canvas.add((row * width * 4) as usize);
            let target = (fb_base as *mut u8).add((row * pitch) as usize);
            core::ptr::copy_nonoverlapping(source, target, (width * 4) as usize);
        }
    }
}

pub(super) fn present_desktop_frame() -> bool {
    if !super::MODE_LFB.load(Ordering::Acquire) {
        return false;
    }
    // Firmware framebuffers are already visible; only an owned VirtIO
    // scanout needs the mirror. Preserve canvas -> session lock ordering.
    if BOOT_GPU_UNAVAILABLE.load(Ordering::Acquire) || BOOT_GPU.lock().is_none() {
        return true;
    }
    let width = super::FB_WIDTH.load(Ordering::Acquire);
    let height = super::FB_HEIGHT.load(Ordering::Acquire);
    let pitch = super::FB_PITCH.load(Ordering::Acquire);
    let fb_base = super::fb_base();
    if fb_base == 0 || width == 0 || height == 0 || pitch == 0 {
        return false;
    }
    let Some(damage) = crate::hal::common::framebuffer_impl::take_damage() else {
        return true; // An idle desktop never copies or submits a frame.
    };
    let mut canvas_guard = ANIMATION_CANVAS.lock();
    let Some((canvas_ptr, _)) = ensure_canvas(&mut canvas_guard, width, height) else {
        crate::hal::common::framebuffer_impl::mark_damage(damage.x, damage.y, damage.width, damage.height);
        return false;
    };
    for row in damage.y..damage.y + damage.height {
        // SAFETY: damage is clipped to the installed framebuffer; the canvas
        // is a persistent tightly packed BGRX allocation of the same geometry.
        unsafe {
            let source = (fb_base as *const u8).add(row as usize * pitch as usize + damage.x as usize * 4);
            let target = canvas_ptr.add((row as usize * width as usize + damage.x as usize) * 4);
            core::ptr::copy_nonoverlapping(source, target, damage.width as usize * 4);
        }
    }
    let mut session_guard = BOOT_GPU.lock();
    let result = session_guard.as_mut().is_some_and(|session| {
        session.gpu.display_geometry() == Some((width, height)) &&
        session.gpu.present_region(&mut session.transport, damage.x, damage.y, damage.width, damage.height)
    });
    if !result {
        crate::hal::common::framebuffer_impl::mark_damage(damage.x, damage.y, damage.width, damage.height);
    }
    result
}

static RAY_FIELD: Spinlock<alloc::vec::Vec<u8>> = Spinlock::new(alloc::vec::Vec::new());
static RAY_FIELD_WIDTH: AtomicU32 = AtomicU32::new(0);
static RAY_FIELD_HEIGHT: AtomicU32 = AtomicU32::new(0);
static RAY_FIELD_ROWS: AtomicU32 = AtomicU32::new(0);
static RAY_FIELD_READY: AtomicBool = AtomicBool::new(false);
static RAY_FIELD_POINTER: AtomicUsize = AtomicUsize::new(0);

const RAY_FIELD_ROWS_PER_FRAME: u32 = u32::MAX;

fn clamp01(value: f32) -> f32 {
    value.max(0.0).min(1.0)
}

fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - clamp01(t);
    1.0 - u * u * u
}

fn ease_in_out_cubic(t: f32) -> f32 {
    let t = clamp01(t);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0) * (-2.0 * t + 2.0) * (-2.0 * t + 2.0) / 2.0
    }
}

fn orb_position(
    index: usize,
    t_ms: u32,
    width: u32,
    height: u32,
    cx: f32,
    cy: f32,
    scale: f32,
) -> (f32, f32) {
    let entry = ORB_ENTRY[index];
    let ex = entry[0] * width as f32;
    let ey = entry[1] * height as f32;
    let rx = ex - cx;
    let ry = ey - cy;
    let r0 = libm::hypotf(rx, ry);
    let pane_dir = [
        -core::f32::consts::FRAC_PI_2 - PI / 4.0,
        -PI / 4.0,
        PI - PI / 4.0,
        PI / 4.0,
    ];
    let theta_start = libm::atan2f(ry, rx);
    let mut delta = pane_dir[index] - theta_start;
    while delta > PI {
        delta -= 2.0 * PI;
    }
    while delta < -PI {
        delta += 2.0 * PI;
    }
    let sweep_in = 2.0 * PI * 2.0 + delta;

    if t_ms <= T_ORB0 {
        return (ex, ey);
    }
    if t_ms < T_ENTRY1 {
        let p = (t_ms - T_ORB0) as f32 / (T_ENTRY1 - T_ORB0) as f32;
        let e = ease_out_cubic(p);
        let r = r0 + (34.0 * scale - r0) * e;
        let theta = theta_start + sweep_in * e;
        return (cx + r * libm::cosf(theta), cy + r * libm::sinf(theta));
    }
    if t_ms < T_DANCE1 {
        let p = (t_ms - T_ENTRY1) as f32 / (T_DANCE1 - T_ENTRY1) as f32;
        let e = ease_in_out_cubic(p);
        let r = 34.0 * scale * (1.0 - e) + 15.0 * scale * e;
        let theta = theta_start + sweep_in + 2.0 * PI * e;
        return (cx + r * libm::cosf(theta), cy + r * libm::sinf(theta));
    }
    if t_ms < T_MERGE1 {
        let p = (t_ms - T_DANCE1) as f32 / (T_MERGE1 - T_DANCE1) as f32;
        let e = ease_out_cubic(p);
        let r = 15.0 * scale * (1.0 - e) + 2.0 * scale * e;
        let theta = theta_start + sweep_in + 2.0 * PI + PI * 0.4 * e;
        return (cx + r * libm::cosf(theta), cy + r * libm::sinf(theta));
    }
    (cx + 1.5 * scale, cy)
}

fn orb_pulse(t: u32, index: usize) -> f32 {
    1.0 + 0.08 * libm::sinf(t as f32 * 0.011 + index as f32 * 1.7)
}

fn orb_alpha(t: u32) -> f32 {
    let t = t as f32;
    let appear = clamp01((t - T_ORB0 as f32) / 350.0);
    let fade = 1.0 - clamp01((t - T_MERGE1 as f32) / (T_BLOOM1 - T_MERGE1) as f32);
    appear * fade
}

fn breath(t: u32) -> f32 {
    if t <= T_BLOOM1 {
        clamp01((t - T_BLOOM0) as f32 / (T_BLOOM1 - T_BLOOM0) as f32)
    } else {
        let phase = ((t - T_BLOOM1) as f32 / BREATH_PERIOD_MS as f32) * 2.0 * PI;
        0.86 + 0.14 * libm::sinf(phase)
    }
}

fn prepare_ray_field(width: u32, height: u32, cx: f32, cy: f32, scale: f32) {
    if width == 0 || height == 0 {
        return;
    }
    if RAY_FIELD_READY.load(Ordering::Acquire)
        && RAY_FIELD_WIDTH.load(Ordering::Acquire) == width
        && RAY_FIELD_HEIGHT.load(Ordering::Acquire) == height
    {
        return;
    }

    let mut ray_field = RAY_FIELD.lock();
    let pixel_count = width as usize * height as usize;
    if RAY_FIELD_WIDTH.load(Ordering::Relaxed) != width
        || RAY_FIELD_HEIGHT.load(Ordering::Relaxed) != height
    {
        ray_field.clear();
        ray_field.resize(pixel_count, 0);
        RAY_FIELD_WIDTH.store(width, Ordering::Release);
        RAY_FIELD_HEIGHT.store(height, Ordering::Release);
        RAY_FIELD_ROWS.store(0, Ordering::Release);
        RAY_FIELD_READY.store(false, Ordering::Release);
    }

    let mut row = RAY_FIELD_ROWS.load(Ordering::Acquire).min(height);
    let row_end = row.saturating_add(RAY_FIELD_ROWS_PER_FRAME).min(height);
    let limit = width.min(height) as f32 * 0.52;
    let denominator = limit * limit * 0.30;
    while row < row_end {
        let dy = row as f32 - cy;
        for x in 0..width {
            let dx = x as f32 - cx;
            let dist2 = dx * dx + dy * dy;
            let along = 1.0 / (1.0 + dist2 / denominator);
            let pd1 = (dy - dx).abs() * 0.707_106_8;
            let w1 = 7.0 * scale + dx.abs() * 0.05;
            let q1 = pd1 / w1;
            let band1 = (1.0 / (1.0 + q1 * q1)) * (1.0 / (1.0 + q1 * q1)) * along;
            let pd2 = (dy + dx).abs() * 0.707_106_8;
            let w2 = 7.0 * scale + dy.abs() * 0.05;
            let q2 = pd2 / w2;
            let band2 = (1.0 / (1.0 + q2 * q2)) * (1.0 / (1.0 + q2 * q2)) * along;
            let field = (band1 + band2) * 0.5;
            ray_field[row as usize * width as usize + x as usize] =
                libm::roundf((field * 255.0).min(255.0)) as u8;
        }
        row += 1;
    }
    RAY_FIELD_ROWS.store(row_end, Ordering::Release);
    if row_end == height {
        RAY_FIELD_POINTER.store(ray_field.as_ptr() as usize, Ordering::Release);
        RAY_FIELD_READY.store(true, Ordering::Release);
    }
}

fn draw_rays(frame: &mut FrameView, _cx: f32, _cy: f32, _scale: f32, t: u32) {
    if t < T_BLOOM0 || !RAY_FIELD_READY.load(Ordering::Acquire) {
        return;
    }
    let (width, height) = (frame.width, frame.height);
    if RAY_FIELD_WIDTH.load(Ordering::Acquire) != width
        || RAY_FIELD_HEIGHT.load(Ordering::Acquire) != height
    {
        return;
    }
    let ray_pointer = RAY_FIELD_POINTER.load(Ordering::Acquire);
    if ray_pointer == 0 {
        return;
    }
    let breath_value = breath(t);
    let ray_alpha = 0.085 * breath_value;
    if ray_alpha < 0.003 {
        return;
    }
    let ray_field = unsafe {
        core::slice::from_raw_parts(ray_pointer as *const u8, width as usize * height as usize)
    };
    let ray_gain_int = (ray_alpha * 510.0) as u32;
    for y in frame.y_start..frame.y_end {
        let row = y as usize * width as usize;
        let pixel_row = unsafe { frame.base.add(row) };
        for x in 0..width {
            let field = ray_field[row + x as usize];
            let scaled = (u32::from(field) * ray_gain_int) / 255;
            if scaled != 0 {
                unsafe {
                    core::ptr::write(
                        pixel_row.add(x as usize),
                        0xFF00_0000 | scaled | (scaled << 8) | (scaled << 16),
                    );
                }
            }
        }
    }
}

fn wave_dy(lx: f32, scale: f32) -> f32 {
    let normal = lx / (FLAG_FW * scale);
    -(11.0 * scale) * libm::sinf(normal * 1.85) + 3.5 * scale * libm::cosf(normal * 3.3)
}

fn draw_flag(frame: &mut FrameView, cx: f32, cy: f32, scale: f32, t: u32) {
    if t < T_BLOOM0 {
        return;
    }
    for pane in 0..4 {
        draw_pane(frame, cx, cy, scale, t, pane);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_pane(frame: &mut FrameView, cx: f32, cy: f32, scale: f32, t: u32, pane: usize) {
    let top = pane < 2;
    let left = pane == 0 || pane == 2;
    let progress = clamp01((t - T_BLOOM0) as f32 / (T_BLOOM1 - T_BLOOM0) as f32);
    let form_ease = ease_out_cubic(progress);
    let unfold = (1.0 - form_ease) * 26.0 * scale;
    let off_x = if left { -unfold } else { unfold } * 0.707_106_8;
    let off_y = if top { -unfold } else { unfold } * 0.707_106_8;
    let form_alpha = clamp01(progress * 1.6);
    if form_alpha <= 0.001 {
        return;
    }
    let (width, height) = (frame.width, frame.height);
    let fw = FLAG_FW * scale;
    let fh = FLAG_FH * scale;
    let gap = FLAG_GAP * scale;
    let pane_w = fw - gap;
    let pane_h = fh - gap;
    let base = ORB_RGB[pane];
    let dirx = if left { -1.0 } else { 1.0 };
    let x0 = cx + off_x + dirx * gap;
    let x1 = cx + off_x + dirx * fw;
    let y0 = cy + off_y - fh - 16.0 * scale;
    let y1 = cy + off_y + fh + 16.0 * scale;
    let xa = (x0.min(x1) as i32 - 1).max(0) as u32;
    let xb = ((x0.max(x1) as i32) + 1).min(width as i32 - 1) as u32;
    let ya = (y0.max(0.0) as u32)
        .min(height.saturating_sub(1))
        .max(frame.y_start);
    let yb = ((y1 + 1.0) as u32).min(height).min(frame.y_end).max(ya);
    let breath_value = breath(t);

    for wy in ya..yb {
        for wx in xa..=xb {
            let lx_s = wx as f32 - cx - off_x;
            let lx_side = if left { -lx_s } else { lx_s };
            if lx_side < gap || lx_side > fw {
                continue;
            }
            let ly0 = wy as f32 - cy - off_y;
            let wave = wave_dy(lx_s, scale);
            let shrink = 1.0 + 0.14 * (lx_s / fw);
            let ly = (ly0 - wave) / shrink;
            let ly_side = if top { -ly } else { ly };
            if ly_side < gap || ly_side > fh {
                continue;
            }
            let edge_x = (lx_side - gap).min(fw - lx_side);
            let edge_y = (ly_side - gap).min(fh - ly_side);
            let aa = clamp01(edge_x.min(edge_y) / 2.0);
            if aa <= 0.0 {
                continue;
            }
            let v = (ly_side - gap) / pane_h;
            let hd = if top { 1.0 - v } else { v };
            let shade = 0.92 + 0.28 * hd;
            let highlight = 0.95 + 0.10 * ((lx_side - gap) / pane_w);
            let mut red = base[0] * shade * highlight;
            let mut green = base[1] * shade * highlight;
            let mut blue = base[2] * shade * highlight;
            let rim = clamp01((0.06 - hd) / 0.06) * 0.35;
            red += (255.0 - red) * rim;
            green += (255.0 - green) * rim;
            blue += (255.0 - blue) * rim;
            let glow = 1.0 + 0.18 * breath_value;
            red = (red * glow).min(255.0);
            green = (green * glow).min(255.0);
            blue = (blue * glow).min(255.0);
            let alpha = aa * form_alpha;
            let alpha_u8 = libm::roundf(alpha * 255.0).min(255.0) as u32;
            if alpha_u8 > 0 {
                frame.blend(wx, wy, blue as u32, green as u32, red as u32, alpha_u8);
            }
        }
    }
}

fn draw_orbs(frame: &mut FrameView, cx: f32, cy: f32, scale: f32, t: u32) {
    if !(T_ORB0..=T_BLOOM1).contains(&t) {
        return;
    }
    let alpha = orb_alpha(t);
    if alpha <= 0.004 {
        return;
    }
    let brighten = 0.55 + 0.45 * clamp01((t - T_ORB0) as f32 / (T_MERGE1 - T_ORB0) as f32);
    let (width, height) = (frame.width, frame.height);
    for index in 0..4 {
        let pulse = orb_pulse(t, index);
        let rgb = ORB_RGB[index];
        for step in 1..15 {
            let Some(trail_t) = t.checked_sub(step * 22) else {
                break;
            };
            if trail_t < T_ORB0 {
                break;
            }
            let (tx, ty) = orb_position(index, trail_t, width, height, cx, cy, scale);
            let age = step as f32 / 14.0;
            let inverse = 1.0 - age;
            let trail_alpha = inverse * inverse * 0.45 * alpha * brighten;
            if trail_alpha <= 0.02 {
                continue;
            }
            let radius = (2.5 + 7.0 * inverse) * scale;
            let trail_rgb = rgb;
            let trail_alpha_value = trail_alpha;
            draw_disk(frame, tx, ty, radius, |distance| {
                let falloff = 1.0 - distance / radius;
                let falloff2 = falloff * falloff;
                let a = falloff2 * trail_alpha_value;
                (
                    trail_rgb[2] * a * 0.90 + 255.0 * a * 0.18,
                    trail_rgb[1] * a * 0.90 + 255.0 * a * 0.18,
                    trail_rgb[0] * a * 0.90 + 255.0 * a * 0.18,
                )
            });
        }

        let (x, y) = orb_position(index, t, width, height, cx, cy, scale);
        let halo_r = 24.0 * scale * pulse;
        let mid_r = 13.0 * scale * pulse;
        let core_r = 7.0 * scale * pulse;
        let orb_rgb = rgb;
        let orb_alpha_value = alpha;
        let brighten_value = brighten;
        draw_disk(frame, x, y, halo_r, |distance| {
            let halo_falloff = 1.0 - distance / halo_r;
            let halo_falloff2 = halo_falloff * halo_falloff;
            let halo = if distance < halo_r {
                halo_falloff2 * halo_falloff * 0.60 * orb_alpha_value * brighten_value
            } else {
                0.0
            };
            let mid_falloff = 1.0 - distance / mid_r;
            let mid_falloff2 = mid_falloff * mid_falloff;
            let mid = if distance < mid_r {
                mid_falloff2 * 0.95 * orb_alpha_value * brighten_value
            } else {
                0.0
            };
            let core_falloff = 1.0 - distance / core_r;
            let core_falloff2 = core_falloff * core_falloff;
            let core = if distance < core_r {
                core_falloff2 * orb_alpha_value * brighten_value
            } else {
                0.0
            };
            (
                orb_rgb[2] * halo * 0.85
                    + 255.0 * halo * 0.30
                    + orb_rgb[2] * mid
                    + 255.0 * mid * 0.20
                    + orb_rgb[2] * 0.22 * core
                    + 255.0 * 0.78 * core,
                orb_rgb[1] * halo * 0.85
                    + 255.0 * halo * 0.30
                    + orb_rgb[1] * mid
                    + 255.0 * mid * 0.20
                    + orb_rgb[1] * 0.22 * core
                    + 255.0 * 0.78 * core,
                orb_rgb[0] * halo * 0.85
                    + 255.0 * halo * 0.30
                    + orb_rgb[0] * mid
                    + 255.0 * mid * 0.20
                    + orb_rgb[0] * 0.22 * core
                    + 255.0 * 0.78 * core,
            )
        });
    }
}

fn draw_flash(frame: &mut FrameView, cx: f32, cy: f32, scale: f32, t: u32) {
    if (T_DANCE1..T_FLASH1).contains(&t) {
        let progress = clamp01((t - T_DANCE1) as f32 / (T_FLASH1 - T_DANCE1) as f32);
        let raw = libm::sinf(progress * PI).max(0.0);
        let intensity = raw * libm::sqrtf(raw);
        let radius = (14.0 + 48.0 * ease_out_cubic(clamp01(progress * 1.4))) * scale;
        let intensity_value = intensity;
        draw_disk(frame, cx, cy, radius, |distance| {
            let falloff = 1.0 - distance / radius;
            let core = falloff * falloff * falloff * intensity_value * 0.70;
            (255.0 * core, 255.0 * core, 255.0 * core)
        });
        if (T_FLASH0..=T_FLASH0 + 300).contains(&t) {
            let quick = (t - T_FLASH0) as f32 / 300.0;
            let wash = libm::sinf(quick * PI).max(0.0) * 0.05;
            let add = 255.0 * wash;
            for y in frame.y_start..frame.y_end {
                let row = (y as usize) * frame.width as usize;
                let pixel_row = unsafe { frame.base.add(row) };
                for x in 0..frame.width {
                    let pixel = unsafe { pixel_row.add(x as usize) };
                    unsafe {
                        let current = core::ptr::read(pixel);
                        let next_blue = ((current & 0xFF) as f32 + add).min(255.0) as u32;
                        let next_green =
                            ((((current >> 8) & 0xFF) as f32 + add).min(255.0) as u32) << 8;
                        let next_red =
                            ((((current >> 16) & 0xFF) as f32 + add).min(255.0) as u32) << 16;
                        core::ptr::write(
                            pixel,
                            (current & 0xFF00_0000) | next_blue | next_green | next_red,
                        );
                    }
                }
            }
        }
    }
}

fn draw_disk<F>(frame: &mut FrameView, cx: f32, cy: f32, radius: f32, mut callback: F)
where
    F: FnMut(f32) -> (f32, f32, f32),
{
    if radius <= 0.0 {
        return;
    }
    let (width, height) = (frame.width, frame.height);
    let x0 = libm::floorf(cx - radius).max(0.0) as u32;
    let x1 = (libm::ceilf(cx + radius) as i32)
        .min(width as i32 - 1)
        .max(0) as u32;
    let y0 = libm::floorf(cy - radius).max(0.0).max(frame.y_start as f32) as u32;
    let y1 = (libm::ceilf(cy + radius) as i32)
        .min(height as i32 - 1)
        .max(0) as u32;
    let y1 = y1.min(frame.y_end.saturating_sub(1)).max(y0);
    if x1 < x0 || y1 < y0 {
        return;
    }
    for y in y0..=y1 {
        let dy = y as f32 - cy;
        let dy2 = dy * dy;
        for x in x0..=x1 {
            let dx = x as f32 - cx;
            let distance2 = dx * dx + dy2;
            if distance2 < radius * radius {
                let distance = libm::sqrtf(distance2);
                let (blue, green, red) = callback(distance);
                frame.add(x, y, blue, green, red);
            }
        }
    }
}

fn add_pixel(x: u32, y: u32, blue: f32, green: f32, red: f32) {
    let base = super::render_base();
    let pitch = super::render_pitch();
    let width = super::FB_WIDTH.load(Ordering::Acquire);
    let height = super::FB_HEIGHT.load(Ordering::Acquire);
    if base == 0 || pitch == 0 || x >= width || y >= height {
        return;
    }
    let offset = ((y * pitch) + (x * 4)) as usize;
    let pixel = unsafe { (base as *mut u8).add(offset) };
    unsafe {
        let current_blue = core::ptr::read_volatile(pixel) as f32;
        let current_green = core::ptr::read_volatile(pixel.add(1)) as f32;
        let current_red = core::ptr::read_volatile(pixel.add(2)) as f32;
        let mixed_blue = (current_blue + blue).min(255.0) as u8;
        let mixed_green = (current_green + green).min(255.0) as u8;
        let mixed_red = (current_red + red).min(255.0) as u8;
        core::ptr::write_volatile(pixel, mixed_blue);
        core::ptr::write_volatile(pixel.add(1), mixed_green);
        core::ptr::write_volatile(pixel.add(2), mixed_red);
        core::ptr::write_volatile(pixel.add(3), 0xFF);
    }
}

fn draw_text(width: u32, height: u32, _cx: f32, _cy: f32, scale: f32, t: u32) {
    let t = t as f32;
    let text_alpha = clamp01((t - T_TEXT0 as f32) / (T_TEXT1 - T_TEXT0) as f32);
    if text_alpha > 0.002 {
        let y = (height as f32 * 0.682) as u32;
        draw_label_bitmap(
            STARTING_LABEL,
            (228, 25),
            width,
            y,
            (text_alpha * 255.0) as u32,
            0xFF,
            0xFF,
            0xFF,
            (26.0 * scale) as u32,
        );
    }
    let copyright_alpha = clamp01((t - T_CPY0 as f32) / (T_CPY1 - T_CPY0) as f32);
    if copyright_alpha > 0.002 {
        let y = (height as f32 * 0.935) as u32;
        draw_label_bitmap(
            COPYRIGHT_LABEL,
            (169, 14),
            width,
            y,
            (copyright_alpha * 255.0) as u32,
            150,
            150,
            150,
            (14.0 * scale) as u32,
        );
    }
}

// Fixed labels are rasterized from the repository's DejaVu Sans face.
// Early boot must not parse and free a complete TrueType font in an ISR.
static STARTING_LABEL: &[u8] = include_bytes!("assets/starting.alpha");
static COPYRIGHT_LABEL: &[u8] = include_bytes!("assets/copyright.alpha");

fn draw_label_bitmap(
    coverage: &[u8],
    dimensions: (u32, u32),
    screen_width: u32,
    y: u32,
    alpha: u32,
    blue: u32,
    green: u32,
    red: u32,
    pixel_height: u32,
) {
    let (source_width, source_height) = dimensions;
    let height = pixel_height.max(1);
    let width = (source_width * height / source_height).max(1);
    let x = screen_width.saturating_sub(width) / 2;
    for row in 0..height {
        for column in 0..width {
            let source = (row * source_height / height * source_width
                + column * source_width / width) as usize;
            let blended = u32::from(coverage[source]) * alpha / 255;
            if blended != 0 {
                super::boot_blend_pixel(x + column, y + row, blue, green, red, blended);
            }
        }
    }
}
