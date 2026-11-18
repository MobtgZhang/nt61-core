//! Limited SP1 apartment initialization for interpreted guest threads.
//! Context values are opaque guest tokens: activation, context interfaces,
//! cross-apartment calls and message pumping are not implemented here.
use super::*;

#[derive(Default)]
pub(super) struct State {
    mta: Option<Mta>,
}

struct Mta {
    context: u64,
    participants: u32,
}

#[derive(Default)]
pub(super) struct ThreadState {
    apartment: Option<Apartment>,
}

struct Apartment {
    sta: bool,
    references: u32,
    context: u64,
}

pub(super) fn cleanup_thread(image: &mut GuestImage) {
    if let Some(apartment) = image.win32.local.com.apartment.take() {
        if !apartment.sta {
            let Some(mta) = image.win32.com.mta.as_mut() else { return; };
            if mta.participants > 1 {
                mta.participants -= 1;
                return;
            }
            image.win32.com.mta = None;
        }
        // STA owns its context; the last MTA participant owns the shared
        // context's release. Repeated initialization is a per-thread count.
        let _ = free(image, apartment.context);
    }
}

pub(super) fn invoke(image: &mut GuestImage, name: &str, a: &[u64]) -> Result<u64, ApiError> {
    match name {
        "CoInitializeEx" => {
            let flags = a[1] as u32;
            if a[0] != 0 || flags & !0x0e != 0 { return Ok(0x80070057); }
            let sta = flags & 2 != 0;
            if let Some(apartment) = image.win32.local.com.apartment.as_mut() {
                if apartment.sta != sta { return Ok(0x80010106); }
                let Some(count) = apartment.references.checked_add(1) else {
                    return Ok(0x8007000e);
                };
                apartment.references = count;
                return Ok(1);
            }
            let existing_mta = if sta { None } else { image.win32.com.mta.as_mut() };
            let context = if let Some(mta) = existing_mta {
                let Some(count) = mta.participants.checked_add(1) else {
                    return Ok(0x8007000e);
                };
                mta.participants = count;
                mta.context
            } else {
                let context = match allocate(image, 0, PAGE as u64, READ) {
                    Ok(pointer) => pointer,
                    Err(_) => return Ok(0x8007000e),
                };
                if !sta {
                    image.win32.com.mta = Some(Mta { context, participants: 1 });
                }
                context
            };
            image.win32.local.com.apartment = Some(Apartment { sta, references: 1, context });
            Ok(0)
        }
        "CoUninitialize" => {
            if let Some(apartment) = image.win32.local.com.apartment.as_mut() {
                if apartment.references > 1 { apartment.references -= 1; }
                else { cleanup_thread(image); }
            }
            // This export is void; zero here is only the interpreter's return
            // register convention, never an HRESULT success assertion.
            Ok(0)
        }
        "CoGetContextToken" => {
            let Some(apartment) = image.win32.local.com.apartment.as_ref() else {
                return Ok(0x800401f0);
            };
            let token = apartment.context;
            let width = if image.thread.context.mode == DecodeMode::X86 { 4 } else { 8 };
            put(&mut image.memory, a[0], token, width).map_err(|_| ApiError::MemoryFault)?;
            Ok(0)
        }
        _ => Err(ApiError::Unsupported),
    }
}
