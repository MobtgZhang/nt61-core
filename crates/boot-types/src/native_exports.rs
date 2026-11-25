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
        name: "RtlAppendStringToString",
        ordinal: 1326,
        target: ExportTarget::Function("nt61_export_RtlAppendStringToString"),
    },
    NativeExport {
        name: "RtlAppendUnicodeStringToString",
        ordinal: 1327,
        target: ExportTarget::Function("nt61_export_RtlAppendUnicodeStringToString"),
    },
    NativeExport {
        name: "RtlAppendUnicodeToString",
        ordinal: 1328,
        target: ExportTarget::Function("nt61_export_RtlAppendUnicodeToString"),
    },
    NativeExport {
        name: "RtlAreBitsClear",
        ordinal: 1331,
        target: ExportTarget::Function("nt61_export_RtlAreBitsClear"),
    },
    NativeExport {
        name: "RtlAreBitsSet",
        ordinal: 1332,
        target: ExportTarget::Function("nt61_export_RtlAreBitsSet"),
    },
    NativeExport {
        name: "RtlCharToInteger",
        ordinal: 1336,
        target: ExportTarget::Function("nt61_export_RtlCharToInteger"),
    },
    NativeExport {
        name: "RtlClearAllBits",
        ordinal: 1338,
        target: ExportTarget::Function("nt61_export_RtlClearAllBits"),
    },
    NativeExport {
        name: "RtlClearBit",
        ordinal: 1339,
        target: ExportTarget::Function("nt61_export_RtlClearBit"),
    },
    NativeExport {
        name: "RtlClearBits",
        ordinal: 1340,
        target: ExportTarget::Function("nt61_export_RtlClearBits"),
    },
    NativeExport {
        name: "RtlCompareMemory",
        ordinal: 1344,
        target: ExportTarget::Function("nt61_export_RtlCompareMemory"),
    },
    NativeExport {
        name: "RtlCompareMemoryUlong",
        ordinal: 1345,
        target: ExportTarget::Function("nt61_export_RtlCompareMemoryUlong"),
    },
    NativeExport {
        name: "RtlCompareString",
        ordinal: 1346,
        target: ExportTarget::Function("nt61_export_RtlCompareString"),
    },
    NativeExport {
        name: "RtlComputeCrc32",
        ordinal: 1351,
        target: ExportTarget::Function("nt61_export_RtlComputeCrc32"),
    },
    NativeExport {
        name: "RtlCopyMemory",
        ordinal: 1356,
        target: ExportTarget::Function("nt61_export_RtlCopyMemory"),
    },
    NativeExport {
        name: "RtlCopyString",
        ordinal: 1361,
        target: ExportTarget::Function("nt61_export_RtlCopyString"),
    },
    NativeExport {
        name: "RtlCopyUnicodeString",
        ordinal: 1362,
        target: ExportTarget::Function("nt61_export_RtlCopyUnicodeString"),
    },
    NativeExport {
        name: "RtlEqualString",
        ordinal: 1402,
        target: ExportTarget::Function("nt61_export_RtlEqualString"),
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
        name: "RtlFindClearBitsAndSet",
        ordinal: 1412,
        target: ExportTarget::Function("nt61_export_RtlFindClearBitsAndSet"),
    },
    NativeExport {
        name: "RtlFindClearRuns",
        ordinal: 1413,
        target: ExportTarget::Function("nt61_export_RtlFindClearRuns"),
    },
    NativeExport {
        name: "RtlFindFirstRunClear",
        ordinal: 1415,
        target: ExportTarget::Function("nt61_export_RtlFindFirstRunClear"),
    },
    NativeExport {
        name: "RtlFindLastBackwardRunClear",
        ordinal: 1416,
        target: ExportTarget::Function("nt61_export_RtlFindLastBackwardRunClear"),
    },
    NativeExport {
        name: "RtlFindLeastSignificantBit",
        ordinal: 1417,
        target: ExportTarget::Function("nt61_export_RtlFindLeastSignificantBit"),
    },
    NativeExport {
        name: "RtlFindLongestRunClear",
        ordinal: 1418,
        target: ExportTarget::Function("nt61_export_RtlFindLongestRunClear"),
    },
    NativeExport {
        name: "RtlFindMostSignificantBit",
        ordinal: 1420,
        target: ExportTarget::Function("nt61_export_RtlFindMostSignificantBit"),
    },
    NativeExport {
        name: "RtlFindNextForwardRunClear",
        ordinal: 1421,
        target: ExportTarget::Function("nt61_export_RtlFindNextForwardRunClear"),
    },
    NativeExport {
        name: "RtlFindSetBits",
        ordinal: 1423,
        target: ExportTarget::Function("nt61_export_RtlFindSetBits"),
    },
    NativeExport {
        name: "RtlFindSetBitsAndClear",
        ordinal: 1424,
        target: ExportTarget::Function("nt61_export_RtlFindSetBitsAndClear"),
    },
    NativeExport {
        name: "RtlInitAnsiString",
        ordinal: 1462,
        target: ExportTarget::Function("nt61_export_RtlInitAnsiString"),
    },
    NativeExport {
        name: "RtlInitString",
        ordinal: 1466,
        target: ExportTarget::Function("nt61_export_RtlInitString"),
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
        name: "RtlIntegerToChar",
        ordinal: 1483,
        target: ExportTarget::Function("nt61_export_RtlIntegerToChar"),
    },
    NativeExport {
        name: "RtlIntegerToUnicode",
        ordinal: 1484,
        target: ExportTarget::Function("nt61_export_RtlIntegerToUnicode"),
    },
    NativeExport {
        name: "RtlIntegerToUnicodeString",
        ordinal: 1485,
        target: ExportTarget::Function("nt61_export_RtlIntegerToUnicodeString"),
    },
    NativeExport {
        name: "RtlMoveMemory",
        ordinal: 1531,
        target: ExportTarget::Function("nt61_export_RtlMoveMemory"),
    },
    NativeExport {
        name: "RtlNumberOfClearBits",
        ordinal: 1540,
        target: ExportTarget::Function("nt61_export_RtlNumberOfClearBits"),
    },
    NativeExport {
        name: "RtlNumberOfSetBits",
        ordinal: 1541,
        target: ExportTarget::Function("nt61_export_RtlNumberOfSetBits"),
    },
    NativeExport {
        name: "RtlNumberOfSetBitsUlongPtr",
        ordinal: 1542,
        target: ExportTarget::Function("nt61_export_RtlNumberOfSetBitsUlongPtr"),
    },
    NativeExport {
        name: "RtlPrefixString",
        ordinal: 1551,
        target: ExportTarget::Function("nt61_export_RtlPrefixString"),
    },
    NativeExport {
        name: "RtlSecondsSince1970ToTime",
        ordinal: 1573,
        target: ExportTarget::Function("nt61_export_RtlSecondsSince1970ToTime"),
    },
    NativeExport {
        name: "RtlSecondsSince1980ToTime",
        ordinal: 1574,
        target: ExportTarget::Function("nt61_export_RtlSecondsSince1980ToTime"),
    },
    NativeExport {
        name: "RtlSetAllBits",
        ordinal: 1577,
        target: ExportTarget::Function("nt61_export_RtlSetAllBits"),
    },
    NativeExport {
        name: "RtlSetBit",
        ordinal: 1578,
        target: ExportTarget::Function("nt61_export_RtlSetBit"),
    },
    NativeExport {
        name: "RtlSetBits",
        ordinal: 1579,
        target: ExportTarget::Function("nt61_export_RtlSetBits"),
    },
    NativeExport {
        name: "RtlTestBit",
        ordinal: 1596,
        target: ExportTarget::Function("nt61_export_RtlTestBit"),
    },
    NativeExport {
        name: "RtlTimeToSecondsSince1970",
        ordinal: 1599,
        target: ExportTarget::Function("nt61_export_RtlTimeToSecondsSince1970"),
    },
    NativeExport {
        name: "RtlTimeToSecondsSince1980",
        ordinal: 1600,
        target: ExportTarget::Function("nt61_export_RtlTimeToSecondsSince1980"),
    },
    NativeExport {
        name: "RtlUnicodeStringToInteger",
        ordinal: 1614,
        target: ExportTarget::Function("nt61_export_RtlUnicodeStringToInteger"),
    },
    NativeExport {
        name: "RtlValidateUnicodeString",
        ordinal: 1638,
        target: ExportTarget::Function("nt61_export_RtlValidateUnicodeString"),
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
