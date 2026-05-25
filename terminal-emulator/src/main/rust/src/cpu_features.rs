// src/cpu_features.rs

/// Detect if the current CPU supports ARM SVE2.
/// Uses Rust's built‑in runtime detection macro.
#[inline]
pub fn has_sve2() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        // The macro expands to a runtime check on supported CPU features.
        std::arch::is_aarch64_feature_detected!("sve2")
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}
