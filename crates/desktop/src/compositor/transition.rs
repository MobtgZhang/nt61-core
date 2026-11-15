//! Time-based login fades. Incremental alpha preserves the previous framebuffer
//! without reading kernel framebuffer memory into Ring 3.
pub fn opacity(elapsed_ms: u64, duration_ms: u64) -> u8 {
    if duration_ms == 0 || elapsed_ms >= duration_ms { return 255; }
    // Smoothstep has zero velocity at both ends.
    let t = elapsed_ms * 255 / duration_ms;
    (t * t * (765 - 2 * t) / (255 * 255)) as u8
}

pub fn incremental_alpha(previous: u8, next: u8) -> u8 {
    if next <= previous { return 0; }
    let remaining = 255 - previous as u32;
    ((next as u32 - previous as u32) * 255).div_ceil(remaining).min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fade_is_monotonic_and_finishes_with_an_opaque_frame() {
        let mut previous = 0;
        let mut accumulated = 0f64;
        for elapsed in (0..=450).step_by(15) {
            let next = opacity(elapsed, 450);
            assert!(next >= previous);
            let alpha = incremental_alpha(previous, next) as f64 / 255.0;
            accumulated += (1.0 - accumulated) * alpha;
            assert!((accumulated - next as f64 / 255.0).abs() < 0.035);
            previous = next;
        }
        assert_eq!(previous, 255);
        assert_eq!(accumulated, 1.0);
        assert_eq!(opacity(u64::MAX, 450), 255);
        assert_eq!(incremental_alpha(255, 255), 0);
    }
}
