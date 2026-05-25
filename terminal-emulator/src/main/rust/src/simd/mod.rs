// src/simd/mod.rs

pub mod scalar;
#[cfg(target_arch = "aarch64")]
pub mod sve2;

use crate::pixel::Pixel8;
use crate::cpu_features;

/// Convert RGBA8 pixels to packed 10‑bit format.
/// Dynamically dispatches to SVE2 implementation when available.
pub fn convert_rgba8_to_rgba10(src: &[Pixel8], dst: &mut [u32]) {
    // On aarch64 we may have SVE2 support; otherwise fall back to scalar.
    #[cfg(target_arch = "aarch64")]
    {
        if cpu_features::has_sve2() {
            // SAFETY: The SVE2 implementation is safe as long as the slices are valid.
            unsafe { sve2::convert_rgba8_to_rgba10_sve2(src, dst) };
            return;
        }
    }
    // Fallback for all other architectures or when SVE2 is unavailable.
    scalar::convert_rgba8_to_rgba10_scalar(src, dst);
}
