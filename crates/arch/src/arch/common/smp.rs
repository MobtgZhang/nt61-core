//! Architecture-common SMP interface.
//
//! This module defines the `SmpArch` trait and `extern "Rust"` declarations
//! that allow architecture-specific SMP implementations to be called from
//! generic kernel code.

/// Trait for architecture-specific SMP operations.
///
/// Implement this trait for each supported architecture (aarch64, riscv64,
/// loongarch64) to provide CPU enumeration and boot operations.
pub trait SmpArch {
    /// Returns the number of available CPUs.
    fn cpu_count() -> u32;

    /// Boot secondary CPUs with the given PML4 PFN.
    ///
    /// # Safety
    /// The caller must ensure the page table is valid and identity-mapped.
    unsafe fn boot_secondary_cpus(pml4_pfn: u64);

    /// Returns the ID of the current CPU.
    fn get_current_cpu_id() -> u32;
}

// extern "Rust" declarations — each architecture provides these.
// The linker resolves them to the architecture-specific implementations.
//
// SAFETY: All functions in this block are inherently unsafe because they
// operate on raw page table PFNs and CPU state. Callers must ensure
// preconditions are met.
unsafe extern "Rust" {
    fn __smp_cpu_count() -> u32;
    /// # Safety
    ///
    /// Caller must ensure every raw-pointer argument is valid and that the accessed memory obeys the kernel aliasing and lifetime rules.
    unsafe fn __smp_boot_secondary(pml4_pfn: u64);
    fn __smp_get_current_cpu_id() -> u32;
}

/// Returns the number of available CPUs.
pub fn cpu_count() -> u32 {
    unsafe { __smp_cpu_count() }
}

/// Boot secondary CPUs with the given PML4 PFN.
///
/// # Safety
/// The caller must ensure the page table is valid and identity-mapped.
pub unsafe fn boot_secondary_cpus(pml4_pfn: u64) {
    __smp_boot_secondary(pml4_pfn);
}

/// Returns the ID of the current CPU.
pub fn get_current_cpu_id() -> u32 {
    // SAFETY: This is inherently unsafe because we're calling a function
    // imported from an unsafe extern block that accesses CPU-specific state.
    unsafe { __smp_get_current_cpu_id() }
}

/// Enabled processor hardware identifiers from ACPI MADT. Logical CPU 0 is
/// always the bootstrap processor; firmware identifiers need not be dense.
#[derive(Clone, Copy)]
pub struct ProcessorTopology {
    pub ids: [u64; 64],
    pub count: usize,
}

impl ProcessorTopology {
    pub const fn empty() -> Self { Self { ids: [0; 64], count: 0 } }
    pub fn add(&mut self, id: u64) {
        if self.count < self.ids.len() && !self.ids[..self.count].contains(&id) {
            self.ids[self.count] = id;
            self.count += 1;
        }
    }
    pub fn bootstrap_first(&mut self, id: u64) {
        if let Some(index) = self.ids[..self.count].iter().position(|&v| v == id) {
            self.ids.swap(0, index);
        } else {
            // Reserve slot zero for the CPU that is actually executing,
            // even if firmware omitted it from an already full table.
            if self.count == self.ids.len() {
                self.ids[self.count - 1] = id;
            } else {
                self.add(id);
            }
            self.ids.swap(0, self.count - 1);
        }
    }
}

/// Parse only enabled GICC (ARM), RINTC (RISC-V), or core PIC
/// (LoongArch) entries. Reject malformed/truncated records as a whole.
pub fn processors_from_madt(bytes: &[u8], entry_kind: u8) -> Option<ProcessorTopology> {
    if bytes.len() < 44 || &bytes[..4] != b"APIC" { return None; }
    let length = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    if length != bytes.len() || bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b)) != 0 { return None; }
    let mut topology = ProcessorTopology::empty();
    let mut offset = 44;
    while offset < length {
        let kind = *bytes.get(offset)?;
        let size = *bytes.get(offset + 1)? as usize;
        if size < 2 { return None; }
        let record = bytes.get(offset..offset.checked_add(size)?)?;
        if kind == entry_kind {
            let (id, flags) = match kind {
                11 if size >= 76 => (
                    u64::from_le_bytes(record[68..76].try_into().ok()?),
                    u32::from_le_bytes(record[12..16].try_into().ok()?),
                ),
                24 if size >= 24 => (
                    u64::from_le_bytes(record[8..16].try_into().ok()?),
                    u32::from_le_bytes(record[4..8].try_into().ok()?),
                ),
                17 if size >= 15 => (
                    u32::from_le_bytes(record[7..11].try_into().ok()?) as u64,
                    u32::from_le_bytes(record[11..15].try_into().ok()?),
                ),
                _ => return None,
            };
            if flags & 1 != 0 { topology.add(id); }
        }
        offset += size;
    }
    Some(topology)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn firmware_processors(kind: u8) -> ProcessorTopology {
    let Some((header, _)) = crate::hal::common::acpi::enumerate_tables().into_iter()
        .find(|(_, signature)| signature == b"APIC") else { return ProcessorTopology::empty(); };
    // enumerate_tables maps and checksum-validates the complete ACPI table.
    unsafe {
        let base = header.cast::<u8>();
        let length = core::ptr::read_unaligned(base.add(4).cast::<u32>()) as usize;
        if !(44..=128 * 1024).contains(&length) { return ProcessorTopology::empty(); }
        processors_from_madt(core::slice::from_raw_parts(base, length), kind)
            .unwrap_or(ProcessorTopology::empty())
    }
}
