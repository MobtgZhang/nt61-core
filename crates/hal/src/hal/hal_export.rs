//! Names of the ABI-reviewed native exports. The runtime wrappers and the image
//! builder share `nt61_boot_types::native_exports`; there is no stub export list.
use nt61_boot_types::native_exports::{
    NativeExport, HAL_EXPORTS as HAL_NATIVE_EXPORTS, NTOS_EXPORTS as NTOS_NATIVE_EXPORTS,
};

const fn names<const N: usize>(exports: &[NativeExport]) -> [&'static str; N] {
    let mut result = [""; N];
    let mut index = 0;
    while index < N {
        result[index] = exports[index].name;
        index += 1;
    }
    result
}

pub const HAL_EXPORTS: &[&str] = &names::<{ HAL_NATIVE_EXPORTS.len() }>(HAL_NATIVE_EXPORTS);
pub const NTOS_EXPORTS: &[&str] = &names::<{ NTOS_NATIVE_EXPORTS.len() }>(NTOS_NATIVE_EXPORTS);

pub fn hal_export_names() -> &'static [&'static str] {
    HAL_EXPORTS
}
pub fn ntos_export_names() -> &'static [&'static str] {
    NTOS_EXPORTS
}
