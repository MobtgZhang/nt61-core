//! Explicit ABI-reviewed native exports. Ordinals match the supplied NT 6.1.7601
//! x64 reference. Non-x64 builds expose the same API through their platform ABI;
//! this is not a claim that Windows 7 shipped binaries for those architectures.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportTarget {
    Function(&'static str),
    Data(&'static str),
    Forwarder(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct NativeExport {
    pub name: &'static str,
    pub ordinal: u16,
    pub target: ExportTarget,
}

pub const NTOS_EXPORTS: &[NativeExport] = &[
    NativeExport {
        name: "RtlCompareMemory",
        ordinal: 1344,
        target: ExportTarget::Function("nt61_export_RtlCompareMemory"),
    },
    NativeExport {
        name: "RtlCopyMemory",
        ordinal: 1356,
        target: ExportTarget::Function("nt61_export_RtlCopyMemory"),
    },
    NativeExport {
        name: "RtlCopyUnicodeString",
        ordinal: 1362,
        target: ExportTarget::Function("nt61_export_RtlCopyUnicodeString"),
    },
    NativeExport {
        name: "RtlFillMemory",
        ordinal: 1409,
        target: ExportTarget::Function("nt61_export_RtlFillMemory"),
    },
    NativeExport {
        name: "RtlFindClearBits",
        ordinal: 1411,
        target: ExportTarget::Function("nt61_export_RtlFindClearBits"),
    },
    NativeExport {
        name: "RtlFindSetBits",
        ordinal: 1423,
        target: ExportTarget::Function("nt61_export_RtlFindSetBits"),
    },
    NativeExport {
        name: "RtlInitAnsiString",
        ordinal: 1462,
        target: ExportTarget::Function("nt61_export_RtlInitAnsiString"),
    },
    NativeExport {
        name: "RtlInitUnicodeString",
        ordinal: 1467,
        target: ExportTarget::Function("nt61_export_RtlInitUnicodeString"),
    },
    NativeExport {
        name: "RtlInitUnicodeStringEx",
        ordinal: 1468,
        target: ExportTarget::Function("nt61_export_RtlInitUnicodeStringEx"),
    },
    NativeExport {
        name: "RtlInitializeBitMap",
        ordinal: 1470,
        target: ExportTarget::Function("nt61_export_RtlInitializeBitMap"),
    },
    NativeExport {
        name: "RtlMoveMemory",
        ordinal: 1531,
        target: ExportTarget::Function("nt61_export_RtlMoveMemory"),
    },
    NativeExport {
        name: "RtlZeroMemory",
        ordinal: 1646,
        target: ExportTarget::Function("nt61_export_RtlZeroMemory"),
    },
];

pub const HAL_EXPORTS: &[NativeExport] = &[NativeExport {
    name: "HalQueryRealTimeClock",
    ordinal: 46,
    target: ExportTarget::Function("nt61_export_HalQueryRealTimeClock"),
}];
