//! Execute the production token constructor and effective-identity checks.
#![allow(dead_code, non_camel_case_types)]
extern crate alloc;
#[path = "../src/se/tokenmgmt/adjust.rs"]
mod adjust;
#[path = "native_security_token/adjust_cases.rs"]
mod adjust_cases;
#[path = "../src/se/acl.rs"]
pub mod acl;
#[path = "../../libs/ntdll/src/ntdll/privilege.rs"]
mod privilege;
#[path = "../src/se/sid.rs"]
pub mod sid;
#[path = "../src/se/token.rs"]
pub mod token;
pub mod se {
    pub use crate::{acl, sid, token};
}
mod mm {
    pub mod pool {
        pub enum PoolType {
            NonPaged,
        }
        pub fn allocate(_: PoolType, size: usize) -> *mut u8 {
            // SAFETY: a valid layout with alignment sufficient for Token/Acl.
            unsafe { std::alloc::alloc(std::alloc::Layout::from_size_align(size, 16).unwrap()) }
        }
        pub fn free(pointer: *mut u8) {
            unsafe {
                std::alloc::dealloc(
                    pointer,
                    std::alloc::Layout::from_size_align(
                        core::mem::size_of::<crate::token::Token>(),
                        16,
                    )
                    .unwrap(),
                );
            }
        }
    }
}
mod ps {
    pub mod process {
        thread_local! { pub static CURRENT: core::cell::Cell<*mut crate::token::Token> = const { core::cell::Cell::new(core::ptr::null_mut()) }; }
        pub struct EffectiveTokenReference(*mut crate::token::Token);
        impl EffectiveTokenReference {
            pub fn as_ref(&self) -> Option<&crate::token::Token> {
                // The test owns CURRENT until this synchronous privilege check returns.
                unsafe { self.0.as_ref() }
            }
        }
        pub fn get_current_thread_token() -> EffectiveTokenReference {
            EffectiveTokenReference(CURRENT.with(|v| v.get()))
        }
    }
}

#[test]
fn system_constructor_assigns_the_system_logon_session() {
    let pointer = token::create_system_token();
    assert!(!pointer.is_null());
    let system = unsafe { &*pointer };
    assert_eq!(system.authentication_id.to_u64(), 0x3e7);
    assert!(system
        .user
        .equals(&sid::sid::well_known(sid::WellKnownSid::LocalSystem)));
    ps::process::CURRENT.with(|v| v.set(pointer));
    assert!(privilege::system_logon());
    ps::process::CURRENT.with(|v| v.set(core::ptr::null_mut()));
    mm::pool::free(pointer.cast());
}

#[test]
fn system_sid_without_system_authentication_does_not_grant_system_logon() {
    let pointer = token::create_user_token(sid::sid::well_known(sid::WellKnownSid::LocalSystem));
    assert!(!pointer.is_null());
    ps::process::CURRENT.with(|v| v.set(pointer));
    assert!(!privilege::system_logon());
    ps::process::CURRENT.with(|v| v.set(core::ptr::null_mut()));
    assert!(!privilege::system_logon());
    assert!(!privilege::enabled(12));
    mm::pool::free(pointer.cast());
}
