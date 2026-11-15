#[path = "../../../libs/ntdll/src/ntdll/file/handles.rs"]
mod handles;
pub(crate) use handles::*;
pub unsafe extern "C" fn nt_close(handle: super::types::HANDLE) -> i32 {
    close_handle(handle).map_or_else(|status| status, |_| 0)
}
