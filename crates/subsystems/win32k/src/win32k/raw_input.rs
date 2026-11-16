//! Native raw input for the keyboard/mouse class backend. IRQ producers only
//! append to a bounded ring. USER message retrieval drains it in syscall context.
//! No HID/touch devices or unsupported registration modes are invented.
use super::window::{input_context::caller_identity, WINDOW_MANAGER};
use crate::arch::common::syscall_frame::SyscallFrame;
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{copy_to_user, probe_user_write, read_user_object, write_user_object};
use alloc::vec::Vec;

const ERROR: u64 = u32::MAX as u64;
const CAPACITY: usize = 256;
const HEADER_SIZE: u32 = 24;
// Keep hardware state updates and ring publication in one producer order,
// even when two class devices report from different processors.
static CAPTURE_INPUT: Spinlock<()> = Spinlock::new(());
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RawRegistration {
    pub page: u16,
    pub usage: u16,
    pub flags: u32,
    pub target: u64,
}
#[derive(Clone, Copy)]
struct Registration {
    process: u64,
    data: RawRegistration,
}
#[derive(Clone, Copy)]
struct Packet {
    bytes: [u8; 48],
    size: usize,
    keyboard: bool,
    key_event: Option<super::input_services::keyboard::KeyboardEvent>,
    time: u32,
}
#[derive(Clone, Copy)]
struct Delivery {
    process: u64,
    thread: u64,
    handle: u64,
    packet: Packet,
    buffered: bool,
}
struct RawState {
    devices: [bool; 2],
    ring: [Option<Packet>; CAPACITY],
    head: usize,
    tail: usize,
    registrations: Vec<Registration>,
    deliveries: [Option<Delivery>; CAPACITY],
    next_handle: u64,
    mouse_buttons: u8,
}
static RAW: Spinlock<RawState> = Spinlock::new(RawState {
    devices: [false; 2],
    ring: [None; CAPACITY],
    head: 0,
    tail: 0,
    registrations: Vec::new(),
    deliveries: [None; CAPACITY],
    next_handle: 0x7100_0000,
    mouse_buttons: 0,
});
pub fn publish_device(keyboard: bool) {
    RAW.lock().devices[usize::from(keyboard)] = true;
}
fn packet(keyboard: bool) -> Packet {
    let mut result = Packet {
        bytes: [0; 48],
        size: if keyboard { 40 } else { 48 },
        keyboard,
        key_event: None,
        time: crate::ke::time::get_tick_count(),
    };
    result.bytes[..4].copy_from_slice(&u32::from(keyboard).to_le_bytes());
    result.bytes[4..8].copy_from_slice(&(result.size as u32).to_le_bytes());
    result.bytes[8..16].copy_from_slice(&(if keyboard { 2u64 } else { 1u64 }).to_le_bytes());
    result
}
impl RawState {
    fn push(&mut self, packet: Packet) {
        let next = (self.head + 1) % CAPACITY;
        if next == self.tail {
            return;
        }
        self.ring[self.head] = Some(packet);
        self.head = next;
    }
}
pub fn capture_mouse(dx: i16, dy: i16, wheel: i16, buttons: u8) {
    capture_pointer(i32::from(dx), i32::from(dy), false, wheel, buttons);
}

/// Absolute coordinates are normalized to 0..65535, as RAWMOUSE requires.
pub fn capture_pointer(x: i32, y: i32, absolute: bool, wheel: i16, buttons: u8) {
    let _capture = CAPTURE_INPUT.lock();
    super::input_services::keyboard::capture_buttons(buttons);
    let mut state = RAW.lock();
    if !state.devices[0] {
        return;
    }
    let mut packet = packet(false);
    packet.bytes[24..26].copy_from_slice(&u16::from(absolute).to_le_bytes());
    let mut flags = 0u16;
    // Linux/port button bits are left, right, middle; RAWMOUSE uses the same order.
    for index in 0..3 {
        let mask = 1 << index;
        if (buttons ^ state.mouse_buttons) & mask != 0 {
            flags |= 1 << (index * 2 + usize::from(buttons & mask == 0));
        }
    }
    state.mouse_buttons = buttons;
    if wheel != 0 {
        flags |= 0x400;
    }
    packet.bytes[28..30].copy_from_slice(&flags.to_le_bytes());
    packet.bytes[30..32].copy_from_slice(&wheel.saturating_mul(120).to_le_bytes());
    packet.bytes[32..36].copy_from_slice(&u32::from(buttons).to_le_bytes());
    packet.bytes[36..40].copy_from_slice(&x.to_le_bytes());
    packet.bytes[40..44].copy_from_slice(&y.to_le_bytes());
    state.push(packet);
}
pub fn capture_keyboard(scan: u8, down: bool) {
    capture_keyboard_extended(scan, down, false);
}

pub fn capture_keyboard_extended(scan: u8, down: bool, extended: bool) {
    let _capture = CAPTURE_INPUT.lock();
    let Some(event) = super::input_services::keyboard::capture_scan(scan, down, extended) else {
        return;
    };
    let mut state = RAW.lock();
    if !state.devices[1] {
        return;
    }
    let mut packet = packet(true);
    packet.key_event = Some(event);
    packet.bytes[24..26].copy_from_slice(&u16::from(scan).to_le_bytes());
    packet.bytes[26..28]
        .copy_from_slice(&(u16::from(!down) | (u16::from(extended) << 1)).to_le_bytes());
    packet.bytes[30..32].copy_from_slice(&event.virtual_key.to_le_bytes());
    let message = match (event.system, down) {
        (false, true) => 0x100u32,
        (false, false) => 0x101,
        (true, true) => 0x104,
        (true, false) => 0x105,
    };
    packet.bytes[32..36].copy_from_slice(&message.to_le_bytes());
    state.push(packet);
}

/// Run outside IRQ and outside the window lock, before PeekMessage/GetMessage.
pub fn drain() {
    let mut notifications = Vec::new();
    let mut keyboard_messages = Vec::new();
    if notifications.try_reserve(CAPACITY).is_err()
        || keyboard_messages.try_reserve(CAPACITY).is_err()
    {
        return;
    }
    {
        let mut state = RAW.lock();
        let manager = WINDOW_MANAGER.lock();
        let foreground = manager.get_window(manager.get_foreground_window());
        while state.tail != state.head {
            let index = state.tail;
            let Some(packet) = state.ring[index].take() else {
                break;
            };
            state.tail = (index + 1) % CAPACITY;
            if let (Some(event), Some(foreground)) = (packet.key_event, foreground) {
                let target = manager
                    .get_window(manager.get_focus())
                    .filter(|window| window.thread_id == foreground.thread_id)
                    .unwrap_or(foreground);
                // RIDEV_NOLEGACY suppresses keyboard messages for that process,
                // while asynchronous state and raw delivery continue to work.
                let no_legacy = state.registrations.iter().any(|registration| {
                    registration.process == target.process_id
                        && registration.data.usage == 6
                        && registration.data.flags & 0x30 == 0x30
                });
                if !no_legacy {
                    let flags = u16::from_le_bytes(packet.bytes[26..28].try_into().unwrap());
                    let scan = u16::from_le_bytes(packet.bytes[24..26].try_into().unwrap());
                    let message = u32::from_le_bytes(packet.bytes[32..36].try_into().unwrap());
                    let lparam = 1
                        | (u32::from(scan) << 16)
                        | (u32::from(flags & 2 != 0) << 24)
                        | (u32::from(event.alt_context) << 29)
                        | (u32::from(event.previous) << 30)
                        | (u32::from(flags & 1 != 0) << 31);
                    keyboard_messages.push((
                        target.thread_id,
                        super::message::Msg {
                            hwnd: target.hwnd,
                            message,
                            wparam: u64::from(event.virtual_key),
                            lparam: i64::from(lparam as i32),
                            time: packet.time,
                            pt_x: 0,
                            pt_y: 0,
                        },
                        event.snapshot,
                    ));
                }
            }
            let registration_count = state.registrations.len();
            for i in 0..registration_count {
                if notifications.len() == CAPACITY {
                    break;
                }
                let registration = state.registrations[i];
                if registration.data.usage != if packet.keyboard { 6 } else { 2 } {
                    continue;
                }
                let target = if registration.data.target != 0 {
                    registration.data.target
                } else {
                    foreground.map_or(0, |window| window.hwnd)
                };
                let Some(window) = manager
                    .get_window(target)
                    .filter(|window| window.process_id == registration.process)
                else {
                    continue;
                };
                let background =
                    foreground.is_none_or(|window| window.process_id != registration.process);
                if background && registration.data.flags & 0x100 == 0 {
                    continue;
                }
                let Some(slot) = state.deliveries.iter().position(Option::is_none) else {
                    continue;
                };
                let Some(handle) = state.next_handle.checked_add(1) else {
                    continue;
                };
                state.next_handle = handle;
                let mut packet = packet;
                packet.bytes[16..24].copy_from_slice(&u64::from(background).to_le_bytes());
                state.deliveries[slot] = Some(Delivery {
                    process: registration.process,
                    thread: window.thread_id,
                    handle,
                    packet,
                    buffered: true,
                });
                notifications.push((window.thread_id, target, u64::from(background), handle));
            }
        }
    }
    for (thread, message, snapshot) in keyboard_messages {
        super::message::post_keyboard_input(thread, message, snapshot);
    }
    for (thread, target, code, handle) in notifications {
        if !super::message::post_thread_message(thread, target, 0xff, code, handle as i64) {
            discard(handle);
        }
    }
}
/// DefWindowProc releases a foreground WM_INPUT packet after processing it.
pub fn release(handle: u64) {
    let owner = caller_identity().0;
    let mut state = RAW.lock();
    if let Some(slot) = state
        .deliveries
        .iter_mut()
        .find(|slot| slot.is_some_and(|d| d.handle == handle && d.process == owner))
    {
        *slot = None;
    }
}
fn discard(handle: u64) {
    let mut state = RAW.lock();
    if let Some(slot) = state
        .deliveries
        .iter_mut()
        .find(|slot| slot.is_some_and(|d| d.handle == handle))
    {
        *slot = None;
    }
}
pub fn dequeued(handle: u64) {
    let owner = caller_identity();
    let mut state = RAW.lock();
    if let Some(delivery) = state
        .deliveries
        .iter_mut()
        .flatten()
        .find(|d| d.handle == handle && (d.process, d.thread) == owner)
    {
        delivery.buffered = false;
    }
}
pub fn release_process(process: u64) {
    let mut state = RAW.lock();
    state
        .registrations
        .retain(|registration| registration.process != process);
    for slot in &mut state.deliveries {
        if slot.is_some_and(|delivery| delivery.process == process) {
            *slot = None;
        }
    }
}
pub fn release_thread(thread: u64) {
    RAW.lock()
        .deliveries
        .iter_mut()
        .filter(|slot| slot.is_some_and(|d| d.thread == thread))
        .for_each(|slot| *slot = None);
}
pub extern "C" fn nt_user_register_raw_input_devices(frame: &SyscallFrame) -> u64 {
    let count = frame.args[1] as u32 as usize;
    if frame.args[2] != 16 || count == 0 || count > 128 {
        return 0;
    }
    let process = caller_identity().0;
    let mut captured = Vec::new();
    if captured.try_reserve_exact(count).is_err() {
        return 0;
    }
    for index in 0..count {
        let Ok(data) =
            read_user_object::<RawRegistration>(frame.args[0].saturating_add(index as u64 * 16))
        else {
            return 0;
        };
        // Keyboard NOLEGACY is handled by the hardware message transport.
        // Mouse NOLEGACY, page exclusions and HID usages remain unsupported.
        if data.page != 1
            || !matches!(data.usage, 2 | 6)
            || data.flags & !(if data.usage == 6 { 0x131 } else { 0x101 }) != 0
            || data.flags & 0x30 != 0 && data.flags & 0x30 != 0x30
            || data.flags & 1 != 0 && data.flags != 1
            || data.flags & 1 != 0 && data.target != 0
            || data.flags & 0x100 != 0 && data.target == 0
        {
            return 0;
        }
        if data.target != 0
            && !WINDOW_MANAGER
                .lock()
                .get_window(data.target)
                .is_some_and(|w| w.process_id == process)
        {
            return 0;
        }
        captured.push(data);
    }
    let mut state = RAW.lock();
    if state.registrations.try_reserve(count).is_err() {
        return 0;
    }
    for data in captured {
        state
            .registrations
            .retain(|r| r.process != process || r.data.usage != data.usage);
        if data.flags & 1 == 0 {
            state.registrations.push(Registration { process, data });
        }
    }
    1
}
pub extern "C" fn nt_user_get_registered_raw_input_devices(frame: &SyscallFrame) -> u64 {
    if frame.args[2] != 16 {
        return ERROR;
    }
    let process = caller_identity().0;
    let state = RAW.lock();
    let needed = state
        .registrations
        .iter()
        .filter(|r| r.process == process)
        .count() as u32;
    let Ok(capacity) = read_user_object::<u32>(frame.args[1]) else {
        return ERROR;
    };
    if frame.args[0] == 0 {
        return if write_user_object(frame.args[1], &needed).is_ok() {
            0
        } else {
            ERROR
        };
    }
    if capacity < needed {
        let _ = write_user_object(frame.args[1], &needed);
        return ERROR;
    }
    if probe_user_write(frame.args[0], needed as usize * 16).is_err() {
        return ERROR;
    }
    for (index, registration) in state
        .registrations
        .iter()
        .filter(|r| r.process == process)
        .enumerate()
    {
        if write_user_object(frame.args[0] + index as u64 * 16, &registration.data).is_err() {
            return ERROR;
        }
    }
    u64::from(needed)
}
pub extern "C" fn nt_user_get_raw_input_device_list(frame: &SyscallFrame) -> u64 {
    if frame.args[2] != 16 {
        return ERROR;
    }
    let state = RAW.lock();
    let needed = state.devices.iter().filter(|&&present| present).count() as u32;
    let Ok(capacity) = read_user_object::<u32>(frame.args[1]) else {
        return ERROR;
    };
    if frame.args[0] == 0 {
        return if write_user_object(frame.args[1], &needed).is_ok() {
            0
        } else {
            ERROR
        };
    }
    if capacity < needed {
        let _ = write_user_object(frame.args[1], &needed);
        return ERROR;
    }
    if probe_user_write(frame.args[0], needed as usize * 16).is_err() {
        return ERROR;
    }
    let mut offset = 0;
    for (index, &present) in state.devices.iter().enumerate() {
        if !present {
            continue;
        }
        let mut data = [0u8; 16];
        data[..8].copy_from_slice(&(index as u64 + 1).to_le_bytes());
        data[8..12].copy_from_slice(&(index as u32).to_le_bytes());
        if copy_to_user(frame.args[0] + offset, &data).is_err() {
            return ERROR;
        }
        offset += 16;
    }
    u64::from(needed)
}
pub extern "C" fn nt_user_get_raw_input_device_info(frame: &SyscallFrame) -> u64 {
    let index = match frame.args[0] {
        1 => 0,
        2 => 1,
        _ => return ERROR,
    };
    if !RAW.lock().devices[index] {
        return ERROR;
    }
    let Ok(capacity) = read_user_object::<u32>(frame.args[3]) else {
        return ERROR;
    };
    let mut data = [0u8; 128];
    let (needed, bytes) = match frame.args[1] as u32 {
        0x20000007 => {
            let name = if index == 0 {
                "\\??\\Root#NT61Mouse#0"
            } else {
                "\\??\\Root#NT61Keyboard#0"
            };
            let mut length = 0;
            for (i, unit) in name.encode_utf16().chain(core::iter::once(0)).enumerate() {
                data[i * 2..i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
                length += 1;
            }
            (length, length * 2)
        }
        0x2000000b => {
            data[..4].copy_from_slice(&32u32.to_le_bytes());
            data[4..8].copy_from_slice(&(index as u32).to_le_bytes());
            if index == 0 {
                data[12..16].copy_from_slice(&3u32.to_le_bytes());
            } else {
                data[8..12].copy_from_slice(&4u32.to_le_bytes());
                data[16..20].copy_from_slice(&12u32.to_le_bytes());
                data[20..24].copy_from_slice(&3u32.to_le_bytes());
                data[24..28].copy_from_slice(&101u32.to_le_bytes());
            }
            (32, 32)
        }
        _ => return ERROR,
    };
    if frame.args[2] == 0 {
        return if write_user_object(frame.args[3], &(needed as u32)).is_ok() {
            0
        } else {
            ERROR
        };
    }
    if capacity < needed as u32 {
        let _ = write_user_object(frame.args[3], &(needed as u32));
        return ERROR;
    }
    if frame.args[1] as u32 == 0x2000000b && read_user_object::<u32>(frame.args[2]).ok() != Some(32)
    {
        return ERROR;
    }
    if copy_to_user(frame.args[2], &data[..bytes]).is_err() {
        return ERROR;
    }
    needed as u64
}
pub extern "C" fn nt_user_get_raw_input_data(frame: &SyscallFrame) -> u64 {
    if frame.args[4] != u64::from(HEADER_SIZE) {
        return ERROR;
    }
    let owner = caller_identity();
    let state = RAW.lock();
    let Some(delivery) = state
        .deliveries
        .iter()
        .flatten()
        .find(|d| d.handle == frame.args[0] && (d.process, d.thread) == owner)
    else {
        return ERROR;
    };
    let needed = match frame.args[1] as u32 {
        0x10000003 => delivery.packet.size,
        0x10000005 => 24,
        _ => return ERROR,
    };
    let Ok(capacity) = read_user_object::<u32>(frame.args[3]) else {
        return ERROR;
    };
    if frame.args[2] == 0 {
        return if write_user_object(frame.args[3], &(needed as u32)).is_ok() {
            0
        } else {
            ERROR
        };
    }
    if capacity < needed as u32 {
        let _ = write_user_object(frame.args[3], &(needed as u32));
        return ERROR;
    }
    if copy_to_user(frame.args[2], &delivery.packet.bytes[..needed]).is_err() {
        return ERROR;
    }
    needed as u64
}
pub extern "C" fn nt_user_get_raw_input_buffer(frame: &SyscallFrame) -> u64 {
    if frame.args[2] != u64::from(HEADER_SIZE) {
        return ERROR;
    }
    drain();
    let owner = caller_identity();
    let mut state = RAW.lock();
    let Ok(capacity) = read_user_object::<u32>(frame.args[1]) else {
        return ERROR;
    };
    let mut indices = [0usize; CAPACITY];
    let mut available = 0;
    for (index, delivery) in state.deliveries.iter().enumerate() {
        if delivery.is_some_and(|d| (d.process, d.thread) == owner && d.buffered) {
            indices[available] = index;
            available += 1;
        }
    }
    indices[..available]
        .sort_unstable_by_key(|&index| state.deliveries[index].map_or(0, |d| d.handle));
    let first_size = indices[..available]
        .first()
        .and_then(|&index| state.deliveries[index])
        .map_or(0, |d| d.packet.size);
    if frame.args[0] == 0 {
        return if write_user_object(frame.args[1], &(first_size as u32)).is_ok() {
            0
        } else {
            ERROR
        };
    }
    if available == 0 {
        return 0;
    }
    if (capacity as usize) < first_size {
        let _ = write_user_object(frame.args[1], &(first_size as u32));
        return ERROR;
    }
    // Validate before consuming any packets. A partial final packet remains
    // queued. Handles establish FIFO order even after delivery slots are reused.
    if probe_user_write(frame.args[0], capacity as usize).is_err() {
        return ERROR;
    }
    let mut offset = 0;
    let mut count = 0;
    for &index in &indices[..available] {
        let Some(delivery) = state.deliveries[index] else {
            continue;
        };
        if offset + delivery.packet.size > capacity as usize {
            break;
        }
        if copy_to_user(
            frame.args[0] + offset as u64,
            &delivery.packet.bytes[..delivery.packet.size],
        )
        .is_err()
        {
            return ERROR;
        }
        offset += delivery.packet.size;
        super::message::remove_raw_input(delivery.thread, delivery.handle);
        state.deliveries[index] = None;
        count += 1;
    }
    count
}
