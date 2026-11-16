//! Boot-local UUID timestamp ranges. A retained random node ID and sequence
//! distinguish allocations across boots without inventing a network address.
//! Persistent sequence storage and RPC/network node selection remain pending.
//! NtAllocateUuids returns NT-epoch time; the RPC layer adds the UUID epoch.
use super::{status::*, types::NTSTATUS};
use crate::ke::sync::Spinlock;
use crate::mm::user_copy::{copy_from_user, copy_to_user, probe_user_write, write_user_object};

const STATUS_RETRY: NTSTATUS = 0xc000022du32 as i32;
const MAX_RANGE: u64 = 10_000;

struct Allocator {
    initialized: bool,
    last_time: u64,
    sequence: u32,
    sequence_changes: u32,
    seed: [u8; 6],
    explicit_seed: bool,
}

static ALLOCATOR: Spinlock<Allocator> = Spinlock::new(Allocator {
    initialized: false, last_time: 0, sequence: 0, sequence_changes: 0,
    seed: [0; 6], explicit_seed: false,
});

impl Allocator {
    fn initialize(&mut self, now: u64) -> Result<(), NTSTATUS> {
        if self.initialized { return Ok(()); }
        let mut bytes = [0u8; 10];
        if !crate::crypto::csprng::fill(&mut bytes) { return Err(STATUS_NO_MEMORY); }
        self.sequence = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        if !self.explicit_seed {
            self.seed.copy_from_slice(&bytes[..6]);
            // Native RPC's local node marker. This must not be represented as
            // a globally assigned IEEE adapter address.
            self.seed[0] |= 0x80;
        }
        self.last_time = now.saturating_sub(MAX_RANGE);
        self.initialized = true;
        Ok(())
    }

    fn allocate(&mut self, now: u64) -> Result<(u64, u32, u32, [u8; 6]), NTSTATUS> {
        self.initialize(now)?;
        if now < self.last_time {
            // A UUID stores only fourteen sequence bits. Refuse to reuse a
            // sequence after a full cycle of backward wall-clock changes.
            if self.sequence_changes == 0x3fff { return Err(STATUS_NO_MEMORY); }
            self.sequence = self.sequence.wrapping_add(1);
            self.sequence_changes += 1;
            self.last_time = now.saturating_sub(MAX_RANGE);
        }
        let range = now.saturating_sub(self.last_time).min(MAX_RANGE);
        if range == 0 { return Err(STATUS_RETRY); }
        let start = self.last_time;
        self.last_time += range;
        Ok((start, range as u32, self.sequence, self.seed))
    }
}

/// # Safety
/// All four output ranges are user addresses, probed before reserving a range.
pub unsafe extern "C" fn nt_allocate_uuids(time: *mut u64, range: *mut u32,
    sequence: *mut u32, seed: *mut u8) -> NTSTATUS {
    let result = (|| {
        for address in [time as u64, range as u64, sequence as u64] {
            if address & 3 != 0 { return Err(0x80000002u32 as i32); }
        }
        probe_user_write(time as u64, 8)?;
        probe_user_write(range as u64, 4)?;
        probe_user_write(sequence as u64, 4)?;
        probe_user_write(seed as u64, 6)?;
        let values = {
            let mut allocator = ALLOCATOR.lock();
            // Serialize sampling with reservation. Sampling before taking this
            // lock can mistake an older CPU's sample for a clock rollback.
            allocator.allocate(crate::ke::time::get_system_time())?
        };
        write_user_object(time as u64, &values.0)?;
        write_user_object(range as u64, &values.1)?;
        write_user_object(sequence as u64, &values.2)?;
        copy_to_user(seed as u64, &values.3)?;
        Ok(STATUS_SUCCESS)
    })();
    result.unwrap_or_else(|status| status)
}

/// # Safety
/// The six-byte node ID is captured after checking the effective logon identity.
pub unsafe extern "C" fn nt_set_uuid_seed(seed: *const u8) -> NTSTATUS {
    if !super::privilege::system_logon() { return STATUS_ACCESS_DENIED; }
    let mut bytes = [0u8; 6];
    if let Err(status) = copy_from_user(&mut bytes, seed as u64) { return status; }
    let mut allocator = ALLOCATOR.lock();
    allocator.seed = bytes;
    allocator.explicit_seed = true;
    STATUS_SUCCESS
}
