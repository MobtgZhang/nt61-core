//! UEFI keyboard input for the Boot Manager.

use alloc::vec::Vec;
use uefi::boot::ScopedProtocol;
use uefi::data_types::Event;
use uefi::proto::console::text::{Input, Key, ScanCode};
use uefi::proto::unsafe_protocol;

/// EFI Simple Text Input Ex protocol GUID.
const SIMPLE_TEXT_INPUT_EX_GUID: uefi::Guid = uefi::guid!("dd9e7534-7762-4698-8c14-f58517a625aa");

/// UEFI EFI_KEY_DATA: ScanCode precedes UnicodeChar on every architecture.
#[repr(C)]
struct ExKeyData {
    scan_code: u16,
    unicode_char: uefi::Char16,
    shift_state: u32,
    toggle_state: u32,
}

/// Minimal definition of `EFI_SIMPLE_TEXT_INPUT_EX_PROTOCOL`.
#[repr(C)]
struct SimpleTextInputExProtocol {
    reset: unsafe extern "efiapi" fn(this: *mut Self, extended_verification: bool) -> uefi::Status,
    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    read_key_stroke_ex:
        unsafe extern "efiapi" fn(this: *mut Self, key: *mut ExKeyData) -> uefi::Status,
    wait_for_key_ex: Event,
}

impl SimpleTextInputExProtocol {
    const GUID: uefi::Guid = SIMPLE_TEXT_INPUT_EX_GUID;
}

/// Wrapper for the extended UEFI keyboard protocol.
#[unsafe_protocol(SimpleTextInputExProtocol::GUID)]
pub struct InputEx(SimpleTextInputExProtocol);

impl InputEx {
    fn reset(&mut self, extended_verification: bool) -> bool {
        // SAFETY: `this` points to the exclusive protocol instance.
        unsafe { (self.0.reset)(&mut self.0, extended_verification) == uefi::Status::SUCCESS }
    }

    fn read_key(&mut self) -> Option<Key> {
        let mut key = ExKeyData {
            unicode_char: uefi::Char16::try_from(0u16).ok()?,
            scan_code: 0,
            shift_state: 0,
            toggle_state: 0,
        };
        let status = unsafe { (self.0.read_key_stroke_ex)(&mut self.0, &mut key) };
        if status != uefi::Status::SUCCESS {
            return None;
        }
        raw_key_to_key((key.unicode_char, key.scan_code))
    }
}

fn raw_key_to_key((unicode_char, scan_code): (uefi::Char16, u16)) -> Option<Key> {
    if scan_code != 0 {
        return Some(Key::Special(ScanCode(scan_code)));
    }
    if u16::from(unicode_char) == 0 {
        return None;
    }
    Some(Key::Printable(unicode_char))
}

/// A keyboard input source. Extended Simple Text Input is preferred because
/// it handles firmware consoles that report only extended key data.
pub struct Keyboard {
    extended: Vec<ScopedProtocol<InputEx>>,
    basic: Vec<ScopedProtocol<Input>>,
    active: Option<ActiveKeyboard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveKeyboard {
    Extended(usize),
    Basic(usize),
}

impl Keyboard {
    pub fn new() -> Option<Self> {
        // Some firmware installs keyboard protocols only after its USB/HID
        // driver is connected to the controller. Recursively connecting every
        // controller is the documented way to ensure keyboard handles exist.
        if let Ok(device_handles) =
            uefi::boot::find_handles::<uefi::proto::device_path::DevicePath>()
        {
            for handle in device_handles {
                let _ = uefi::boot::connect_controller(handle, None, None, true);
            }
        }

        let mut extended = match uefi::boot::find_handles::<InputEx>() {
            Ok(handles) => handles
                .into_iter()
                .filter_map(|handle| uefi::boot::open_protocol_exclusive::<InputEx>(handle).ok())
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        let mut basic = match uefi::boot::find_handles::<Input>() {
            Ok(handles) => handles
                .into_iter()
                .filter_map(|handle| uefi::boot::open_protocol_exclusive::<Input>(handle).ok())
                .collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };

        // Firmware USB stack implementations are not required to start
        // delivering key strokes until the console input protocol has been
        // explicitly reset. Resetting all instances is harmless and lets a
        // secondary console remain usable if the primary is disabled.
        extended.iter_mut().for_each(|input| {
            input.reset(false);
        });
        basic.iter_mut().for_each(|input| {
            let _ = input.reset(false);
        });

        let active = if !extended.is_empty() {
            ActiveKeyboard::Extended(0)
        } else if !basic.is_empty() {
            ActiveKeyboard::Basic(0)
        } else {
            return None;
        };
        crate::debug_println!(
            "[BOOT] keyboard handles: extended={} basic={}",
            extended.len(),
            basic.len()
        );

        Some(Self {
            extended,
            basic,
            active: Some(active),
        })
    }

    pub fn read_key(&mut self) -> Option<Key> {
        if let Some(source) = self.active {
            if let Some(key) = self.read_source(source) {
                return Some(key);
            }
        }
        // Console splitter, serial and USB/virtio handles may all coexist.
        // Poll every source, including after a timer or spurious event.
        for index in 0..self.extended.len() {
            let source = ActiveKeyboard::Extended(index);
            if let Some(key) = self.read_source(source) {
                self.active = Some(source);
                return Some(key);
            }
        }
        for index in 0..self.basic.len() {
            let source = ActiveKeyboard::Basic(index);
            if let Some(key) = self.read_source(source) {
                self.active = Some(source);
                return Some(key);
            }
        }
        None
    }

    fn read_source(&mut self, source: ActiveKeyboard) -> Option<Key> {
        match source {
            ActiveKeyboard::Extended(index) => self
                .extended
                .get_mut(index)
                .and_then(|input| input.read_key()),
            ActiveKeyboard::Basic(index) => self
                .basic
                .get_mut(index)
                .and_then(|input| input.read_key().ok().flatten()),
        }
    }
}
