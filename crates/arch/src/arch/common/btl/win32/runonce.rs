//! SP1 run-once state transitions in checked guest memory. Wait queues belong
//! to the guest scheduler; original keyed-event list pointers are unsupported.
use super::*;

const PENDING: u32 = 0x103;
const UNSUCCESSFUL: u32 = 0xc000_0001;
const INVALID_FLAGS: u32 = 0xc000_00f0;
const INVALID_CONTEXT: u32 = 0xc000_00f1;

pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "RtlRunOnceInitialize"
            | "RtlRunOnceBeginInitialize"
            | "RtlRunOnceComplete"
            | "RtlRunOnceExecuteOnce"
            | "InitOnceInitialize"
            | "InitOnceBeginInitialize"
            | "InitOnceComplete"
            | "InitOnceExecuteOnce"
    )
}

fn width(image: &GuestImage) -> usize {
    if image.thread.context.mode == DecodeMode::X86 {
        4
    } else {
        8
    }
}
fn load(image: &mut GuestImage, address: u64) -> Result<u64, ApiError> {
    let n = width(image);
    word(&mut image.memory, address, n).map_err(|_| ApiError::MemoryFault)
}
fn store(image: &mut GuestImage, address: u64, value: u64) -> Result<(), ApiError> {
    let n = width(image);
    if address == 0 || !image.memory.probe_write(address, n) {
        return Err(ApiError::MemoryFault);
    }
    put(&mut image.memory, address, value, n).map_err(|_| ApiError::MemoryFault)
}

fn begin(
    image: &mut GuestImage,
    api: &catalog::Api,
    address: u64,
    flags: u32,
    output: u64,
) -> Result<u32, ApiError> {
    if !matches!(flags, 0 | 1 | 2) {
        return Ok(INVALID_FLAGS);
    }
    let value = load(image, address)?;
    if value & 3 == 2 {
        if output != 0 {
            store(image, output, value & !3)?;
        }
        return Ok(0);
    }
    if flags == 1 {
        return Ok(UNSUCCESSFUL);
    }
    match value & 3 {
        0 => {
            store(image, address, if flags == 2 { 3 } else { 1 })?;
            Ok(PENDING)
        }
        1 if flags == 0 => {
            if value != 1 {
                return Err(ApiError::Unsupported);
            }
            let target =
                super::super::threads::Target::RunOnce(address, catalog::GATEWAY_BASE | api.id);
            Err(ApiError::PendingRetry(super::super::threads::Wait::lock(
                target,
            )))
        }
        3 if flags == 2 => Ok(PENDING),
        _ => Ok(INVALID_FLAGS),
    }
}

fn complete(
    image: &mut GuestImage,
    address: u64,
    flags: u32,
    context: u64,
) -> Result<u32, ApiError> {
    if !matches!(flags, 0 | 2 | 4) {
        return Ok(INVALID_FLAGS);
    }
    if context & 3 != 0 || (flags == 4 && context != 0) {
        return Ok(INVALID_CONTEXT);
    }
    let value = load(image, address)?;
    match value & 3 {
        1 if flags != 2 => {
            if value != 1 {
                return Err(ApiError::Unsupported);
            }
            store(image, address, if flags == 4 { 0 } else { context | 2 })?;
            Ok(0)
        }
        3 if flags == 2 => {
            store(image, address, context | 2)?;
            Ok(0)
        }
        1 | 3 => Ok(INVALID_FLAGS),
        _ => Ok(UNSUCCESSFUL),
    }
}

fn win32_result(image: &mut GuestImage, status: u32) -> u64 {
    if status as i32 >= 0 {
        return 1;
    }
    error(
        image,
        match status {
            INVALID_FLAGS | INVALID_CONTEXT => 87,
            UNSUCCESSFUL => 31,
            _ => status_error(status),
        },
    );
    0
}

pub(super) fn invoke(
    image: &mut GuestImage,
    api: &catalog::Api,
    a: &[u64],
) -> Result<u64, ApiError> {
    let native = api.dll == "ntdll.dll";
    if api.name.ends_with("Initialize") && !api.name.ends_with("BeginInitialize") {
        store(image, a[0], 0)?;
        return Ok(0);
    }
    if api.name.ends_with("BeginInitialize") {
        let output = if native { a[2] } else { a[3] };
        let status = begin(image, api, a[0], a[1] as u32, output)?;
        if native {
            return Ok(u64::from(status));
        }
        if status as i32 >= 0 {
            put(&mut image.memory, a[2], u64::from(status == PENDING), 4)
                .map_err(|_| ApiError::MemoryFault)?;
        }
        return Ok(win32_result(image, status));
    }
    if api.name.ends_with("Complete") {
        let status = complete(image, a[0], a[1] as u32, a[2])?;
        return Ok(if native {
            u64::from(status)
        } else {
            win32_result(image, status)
        });
    }
    let status = begin(image, api, a[0], 0, a[3])?;
    if status == PENDING {
        image.win32.local.user.completion = Some(user::Completion::RunOnce {
            address: a[0],
            output: a[3],
            native,
        });
        image.win32.local.user.callback = Some((a[1], [a[0], a[2], a[3], 0]));
        Ok(0)
    } else {
        // SP1 ExecuteOnce converts NT_SUCCESS to BOOL without changing LastError.
        Ok(if native {
            u64::from(status)
        } else {
            u64::from(status as i32 >= 0)
        })
    }
}

pub(super) fn callback_complete(
    image: &mut GuestImage,
    address: u64,
    output: u64,
    native: bool,
    result: u64,
) -> Result<u64, ApiError> {
    let status = if result as u32 == 0 {
        let status = complete(image, address, 4, 0)?;
        if status == 0 {
            UNSUCCESSFUL
        } else {
            status
        }
    } else {
        let context = if output == 0 { 0 } else { load(image, output)? };
        let status = complete(image, address, 0, context)?;
        // Original ExecuteOnce raises a status exception if completion fails.
        // Guest exception dispatch is unavailable; stop explicitly.
        if status != 0 {
            return Err(ApiError::Unsupported);
        }
        status
    };
    Ok(if native {
        u64::from(status)
    } else {
        u64::from(status as i32 >= 0)
    })
}
