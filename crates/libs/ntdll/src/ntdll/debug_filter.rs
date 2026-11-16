//! Win7 DPFLTR masks, shared by the query/set syscalls and DbgPrintEx.
use core::sync::atomic::{AtomicU32, Ordering};

const DEFAULT_COMPONENT: usize = 101;
const COMPONENT_COUNT: usize = 147;
const DIRECT_MASK: u32 = 0x8000_0000;
static COMPONENTS: [AtomicU32; COMPONENT_COUNT] =
    [const { AtomicU32::new(0) }; COMPONENT_COUNT];
// Compatibility mask applied to every component; error messages are enabled
// initially, as in the Windows kernel's WIN2000 mask.
static WIN2000: AtomicU32 = AtomicU32::new(1);

fn mask(component: u32) -> &'static AtomicU32 {
    if component == u32::MAX {
        &WIN2000
    } else {
        COMPONENTS.get(component as usize).unwrap_or(&COMPONENTS[DEFAULT_COMPONENT])
    }
}

fn level_bits(level: u32) -> u32 {
    let bits = if level < 32 { 1u32 << level } else { level };
    bits & !DIRECT_MASK
}

pub extern "C" fn nt_query_debug_filter_state(component: u32, level: u32) -> i32 {
    let enabled = mask(component).load(Ordering::Relaxed) | WIN2000.load(Ordering::Relaxed);
    i32::from(enabled & level_bits(level) != 0)
}

pub extern "C" fn nt_set_debug_filter_state(component: u32, level: u32, state: u8) -> i32 {
    if !super::privilege::enabled(20) {
        return super::status::STATUS_ACCESS_DENIED;
    }
    let bits = level_bits(level);
    if state != 0 {
        mask(component).fetch_or(bits, Ordering::Relaxed);
    } else {
        mask(component).fetch_and(!bits, Ordering::Relaxed);
    }
    super::status::STATUS_SUCCESS
}
