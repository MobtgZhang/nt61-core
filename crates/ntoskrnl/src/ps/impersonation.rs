//! Owned, immutable kernel impersonation snapshots and lifetime-pinned readers.
//!
//! This is a prerequisite for impersonation services. It does not implement
//! PsImpersonateClient policy, token object security, or a native entry point.
use crate::ke::sync::Spinlock;
use crate::se::token::{ImpersonationLevel, Token, TokenTypeEnum};
use core::ptr::NonNull;
use core::sync::atomic::{fence, AtomicUsize, Ordering};

struct TokenAllocation {
    references: AtomicUsize,
    token: *mut Token,
}

/// A kernel-owned token snapshot. No mutable token access or native handle is
/// exposed; callers modifying the source cannot invalidate the copied DACL.
pub struct ImpersonationTokenReference(NonNull<TokenAllocation>);
// SAFETY: the allocation and copied token/DACL are immutable after publication;
// only the atomic reference count is mutated through shared references.
unsafe impl Send for ImpersonationTokenReference {}
unsafe impl Sync for ImpersonationTokenReference {}

impl ImpersonationTokenReference {
    pub fn copy_from(source: &Token) -> Result<Self, ImpersonationError> {
        if source.token_type != TokenTypeEnum::TokenImpersonation {
            return Err(ImpersonationError::WrongTokenType);
        }
        Self::copy_for_worker(source)
    }

    /// Snapshot an effective caller token for a kernel worker. Primary tokens
    /// become impersonation tokens; existing impersonation levels are preserved.
    pub(crate) fn copy_for_worker(source: &Token) -> Result<Self, ImpersonationError> {
        let token = crate::se::token::duplicate_for_process(source);
        if token.is_null() {
            return Err(ImpersonationError::NoMemory);
        }
        // SAFETY: duplicate_for_process returned an exclusively owned token.
        unsafe {
            (*token).token_type = TokenTypeEnum::TokenImpersonation;
        }
        let allocation = crate::mm::pool::allocate(
            crate::mm::pool::PoolType::NonPaged,
            core::mem::size_of::<TokenAllocation>(),
        ) as *mut TokenAllocation;
        let Some(allocation) = NonNull::new(allocation) else {
            // SAFETY: duplicate_for_process transferred exclusive ownership.
            unsafe { free_token(token) };
            return Err(ImpersonationError::NoMemory);
        };
        // SAFETY: the fresh, aligned pool allocation is not yet published.
        unsafe {
            allocation.as_ptr().write(TokenAllocation {
                references: AtomicUsize::new(1),
                token,
            })
        };
        Ok(Self(allocation))
    }

    pub fn token(&self) -> &Token {
        // SAFETY: this reference retains the immutable allocation and its token.
        unsafe { &*self.0.as_ref().token }
    }
}

impl Clone for ImpersonationTokenReference {
    fn clone(&self) -> Self {
        // SAFETY: a live reference keeps the allocation initialized.
        let references = unsafe { &self.0.as_ref().references };
        let mut count = references.load(Ordering::Relaxed);
        loop {
            let next = count
                .checked_add(1)
                .filter(|next| *next <= isize::MAX as usize)
                .expect("impersonation token reference overflow");
            match references.compare_exchange_weak(
                count,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => count = actual,
            }
        }
        Self(self.0)
    }
}

impl Drop for ImpersonationTokenReference {
    fn drop(&mut self) {
        // SAFETY: this reference owns one count; the final decrement exclusively
        // owns token and allocation after synchronizing with previous releases.
        unsafe {
            if self.0.as_ref().references.fetch_sub(1, Ordering::Release) == 1 {
                fence(Ordering::Acquire);
                free_token(self.0.as_ref().token);
                crate::mm::pool::free(self.0.as_ptr().cast());
            }
        }
    }
}

unsafe fn free_token(token: *mut Token) {
    unsafe {
        if !(*token).default_dacl.is_null() {
            crate::mm::pool::free((*token).default_dacl.cast());
        }
        crate::mm::pool::free(token.cast());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpersonationError {
    WrongTokenType,
    NoMemory,
    Terminating,
    NoCurrentThread,
}

/// Run trusted kernel work under an owned caller identity on the current
/// thread. Keep ETHREAD's impersonation field private across crate boundaries.
/// The scheduler retains the executing ETHREAD for this call, including yields.
pub fn with_current_thread_token<T>(
    token: ImpersonationReference,
    action: impl FnOnce() -> T,
) -> Result<T, ImpersonationError> {
    let thread = crate::ps::thread::get_current_ethread();
    if thread.is_null() {
        return Err(ImpersonationError::NoCurrentThread);
    }
    // SAFETY: only the live current thread is used; with_token pins the owned
    // identity and restores the previous snapshot before this call returns.
    unsafe { (*thread).impersonation.with_token(token, action) }
}

#[derive(Clone)]
pub struct ImpersonationReference {
    pub token: ImpersonationTokenReference,
    pub level: ImpersonationLevel,
    pub copy_on_open: bool,
    pub effective_only: bool,
}

struct State {
    token: Option<ImpersonationReference>,
    terminating: bool,
}

/// Project ETHREAD extension; all replacement, revert and termination operations
/// serialize with pin acquisition. Destruction is always outside the lock.
pub struct ThreadImpersonation {
    state: Spinlock<State>,
}

impl ThreadImpersonation {
    pub const fn new() -> Self {
        Self {
            state: Spinlock::new(State {
                token: None,
                terminating: false,
            }),
        }
    }

    pub fn reference(&self) -> Option<ImpersonationReference> {
        self.state.lock().token.clone()
    }

    pub fn replace(&self, token: ImpersonationReference) -> Result<(), ImpersonationError> {
        let previous = {
            let mut state = self.state.lock();
            if state.terminating {
                return Err(ImpersonationError::Terminating);
            }
            state.token.replace(token)
        };
        drop(previous);
        Ok(())
    }

    /// Execute under an owned caller snapshot, restoring the previous identity
    /// even on an early return or unwind. Termination never reinstalls a token.
    pub(crate) fn with_token<T>(
        &self,
        token: ImpersonationReference,
        action: impl FnOnce() -> T,
    ) -> Result<T, ImpersonationError> {
        struct Restore<'a> {
            owner: &'a ThreadImpersonation,
            previous: Option<ImpersonationReference>,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                let replaced = {
                    let mut state = self.owner.state.lock();
                    if state.terminating {
                        None
                    } else {
                        core::mem::replace(&mut state.token, self.previous.take())
                    }
                };
                drop(replaced);
            }
        }
        let previous = {
            let mut state = self.state.lock();
            if state.terminating {
                return Err(ImpersonationError::Terminating);
            }
            state.token.replace(token)
        };
        let _restore = Restore { owner: self, previous };
        Ok(action())
    }

    pub fn revert(&self) {
        let previous = { self.state.lock().token.take() };
        drop(previous);
    }

    pub fn terminate(&self) {
        let previous = {
            let mut state = self.state.lock();
            state.terminating = true;
            state.token.take()
        };
        drop(previous);
    }
}

impl Drop for ThreadImpersonation {
    fn drop(&mut self) {
        self.terminate();
    }
}
