pub(crate) const EPOCH_1970: i64 = 116_444_736_000_000_000;
pub(crate) const EPOCH_1980: i64 = 119_600_064_000_000_000;
const TICKS_PER_SECOND: i64 = 10_000_000;

pub(crate) fn to_seconds(time: i64, epoch: i64) -> Option<u32> {
    let elapsed = time.checked_sub(epoch)?;
    if elapsed < 0 {
        return None;
    }
    u32::try_from(elapsed / TICKS_PER_SECOND).ok()
}

pub(crate) fn from_seconds(seconds: u32, epoch: i64) -> i64 {
    epoch + i64::from(seconds) * TICKS_PER_SECOND
}
