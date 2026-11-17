//! Actual production transition owner with explicit persistence boundaries.
extern crate alloc;
#[path = "../../libs/ntdll/src/ntdll/locale/install_language.rs"]
mod production;
use production::InstallLanguage;
const FAILED: i32 = 0xc000_0001u32 as i32;
const MISSING: i32 = 0xc000_0034u32 as i32;

#[test]
fn initially_uncommitted_and_same_language_commit_needs_no_registry_write() {
    let state = InstallLanguage::new(0x409);
    assert_eq!(state.language(), 0x409);
    assert_eq!(state.committed_status(), MISSING);
    assert_eq!(
        state.flush(true, 0x409, 1, |_| panic!("unchanged language")),
        0
    );
    assert_eq!(state.committed_status(), 0);
}

#[test]
fn kernel_mode_rejection_precedes_comparison_and_all_mutation() {
    for language in [0x409, 0x804, 0x1234_0409] {
        let state = InstallLanguage::new(0x409);
        assert_eq!(
            state.flush(false, language, 1, |_| panic!("kernel caller")),
            FAILED
        );
        assert_eq!(state.language(), 0x409);
        assert_eq!(state.committed_status(), MISSING);
    }
}

#[test]
fn uncommitted_input_truncates_language_but_committed_comparison_uses_full_dword() {
    let state = InstallLanguage::new(0x409);
    assert_eq!(
        state.flush(true, 0xabcd_0804, 1, |language| {
            assert_eq!(language, 0x804);
            0
        }),
        0
    );
    assert_eq!(state.language(), 0x804);
    assert_eq!(
        state.flush(true, 0xabcd_0804, 0, |_| panic!("committed")),
        FAILED
    );
    assert_eq!(state.flush(true, 0x804, 0, |_| panic!("committed")), 0);
    assert_eq!(state.committed_status(), 0);
}

#[test]
fn failed_persistence_keeps_new_language_and_commit_and_never_retries_same_value() {
    let state = InstallLanguage::new(0x409);
    assert_eq!(state.flush(true, 0x804, 0, |_| MISSING), MISSING);
    assert_eq!(state.language(), 0x804);
    assert_eq!(state.committed_status(), MISSING);
    assert_eq!(
        state.flush(true, 0x804, 0, |_| panic!("no same-value retry")),
        0
    );
    assert_eq!(state.flush(true, 0x411, 0x100, |_| FAILED), FAILED);
    assert_eq!(state.language(), 0x411);
    assert_eq!(state.committed_status(), 0);
    assert_eq!(state.flush(true, 0x409, 0, |_| panic!("committed")), FAILED);
}

#[test]
fn unchanged_language_accepts_high_bits_before_commit() {
    let state = InstallLanguage::new(0x409);
    assert_eq!(
        state.flush(true, 0xabcd_0409, 0, |_| panic!("unchanged")),
        0
    );
    assert_eq!(state.committed_status(), MISSING);
    assert_eq!(
        state.flush(true, 0xabcd_0409, 0x8000_0000, |_| panic!("unchanged")),
        0
    );
    assert_eq!(state.committed_status(), 0);
    assert_eq!(
        state.flush(true, 0xabcd_0409, 0, |_| panic!("committed")),
        FAILED
    );
}

#[test]
fn queries_observe_state_before_persistence_completes() {
    let state = InstallLanguage::new(0x409);
    assert_eq!(
        state.flush(true, 0x804, 1, |_| {
            assert_eq!(state.language(), 0x804);
            assert_eq!(state.committed_status(), 0);
            MISSING
        }),
        MISSING
    );
}

#[test]
fn changed_install_updates_mui_cache_but_same_install_preserves_initial_override() {
    let state = InstallLanguage::with_languages(0x409, 0x411);
    assert_eq!(state.mui_fallback_language(), 0x411);
    assert_eq!(state.flush(true, 0x409, 0, |_| panic!("same install")), 0);
    assert_eq!(state.mui_fallback_language(), 0x411);
    assert_eq!(
        state.flush(true, 0x804, 0, |_| {
            assert_eq!(state.mui_fallback_language(), 0x804);
            MISSING
        }),
        MISSING
    );
    assert_eq!(state.mui_fallback_language(), 0x804);
}

#[test]
fn global_install_language_starts_with_the_uncommitted_windows_default() {
    assert_eq!(production::INSTALL_LANGUAGE.language(), 0x409);
    assert_eq!(production::INSTALL_LANGUAGE.committed_status(), MISSING);
}
