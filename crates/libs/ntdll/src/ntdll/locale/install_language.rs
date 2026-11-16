//! Install-language state shared by flush and query entry points.
//!
//! The original Win7 flush publishes both its committed flag and new language
//! before registry I/O. A registry failure therefore does not undo either.
use ::spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

const STATUS_SUCCESS: i32 = 0;
const STATUS_UNSUCCESSFUL: i32 = 0xc000_0001u32 as i32;
const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xc000_0034u32 as i32;

pub struct InstallLanguage {
    flush_lock: Mutex<()>,
    language: AtomicU16,
    mui_fallback: AtomicU16,
    committed: AtomicBool,
}

impl InstallLanguage {
    pub const fn new(language: u16) -> Self {
        Self::with_languages(language, language)
    }

    /// The original boot initialization may obtain a different MUI fallback from
    /// MUI\Settings / PreferredUILanguages. A missing cache uses the install
    /// language; callers loading that configuration can preserve both words.
    pub const fn with_languages(language: u16, mui_fallback: u16) -> Self {
        Self {
            flush_lock: Mutex::new(()),
            language: AtomicU16::new(language),
            mui_fallback: AtomicU16::new(mui_fallback),
            committed: AtomicBool::new(false),
        }
    }

    pub fn language(&self) -> u16 {
        self.language.load(Ordering::Acquire)
    }

    /// Cache used by the original kernel's MUI/resource-language selection.
    /// Flush updates it only when the installed language actually changes.
    pub fn mui_fallback_language(&self) -> u16 {
        self.mui_fallback.load(Ordering::Acquire)
    }

    /// NtIsUILanguageComitted returns an NTSTATUS, not a BOOLEAN.
    pub fn committed_status(&self) -> i32 {
        if self.committed.load(Ordering::Acquire) {
            STATUS_SUCCESS
        } else {
            STATUS_OBJECT_NAME_NOT_FOUND
        }
    }

    /// Execute the flush state transition, invoking the real registry operation
    /// only when the truncated language changes. `user_mode` is the trusted
    /// dispatch caller mode, never an input supplied by the user.
    ///
    /// `persist` executes under the flush serialization lock and must not call
    /// `flush` recursively. Queries can observe the published state during I/O.
    /// It must use the kernel registry path, preserve failed-write status, and
    /// return only after its own key lifetime is released.
    pub fn flush(
        &self,
        user_mode: bool,
        language: u32,
        commit: u32,
        persist: impl FnOnce(u16) -> i32,
    ) -> i32 {
        if !user_mode {
            return STATUS_UNSUCCESSFUL;
        }
        if self.committed.load(Ordering::Acquire) {
            return if language == u32::from(self.language()) {
                STATUS_SUCCESS
            } else {
                STATUS_UNSUCCESSFUL
            };
        }
        let _flush = self.flush_lock.lock();
        // Do not reject a racing flush which passed the entry check before
        // another caller committed. The original does not recheck under its
        // exclusive resource. Its flag remains set if the second input is zero.
        // This lock does not raise IRQL or disable interrupts. The persistence
        // implementation currently performs bounded in-memory registry work.
        if commit != 0 {
            self.committed.store(true, Ordering::Release);
        }
        let language = language as u16;
        if self.language() == language {
            return STATUS_SUCCESS;
        }
        self.language.store(language, Ordering::Release);
        self.mui_fallback.store(language, Ordering::Release);
        persist(language)
    }
}

/// Initial install language for the current project's bootstrap configuration.
/// Mount/registry initialization must supply the installed image's LANGID when
/// configurations other than that bootstrap image are supported.
pub static INSTALL_LANGUAGE: InstallLanguage = InstallLanguage::new(0x409);
