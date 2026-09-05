//! Vulkan API policy, kept dependency-free for a host rustc test harness.
const VULKAN_1_1: u32 = (1 << 22) | (1 << 12);

pub(crate) fn max_api_version(created: u32, experiment: bool, property: Option<&str>) -> u32 {
    if experiment && property == Some("0") { 0 } else { created }
}

pub(crate) fn supported(created: u32, loader: u32, physical: u32) -> bool {
    [created, loader, physical].into_iter().all(|v| v >= VULKAN_1_1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_caps_actual_created_not_driver_max() {
        for version in [VULKAN_1_1, VULKAN_1_1 + (1 << 12)] {
            assert_eq!(max_api_version(version, false, None), version);
            assert_eq!(max_api_version(version, true, None), version);
        }
    }

    #[test]
    fn uncapped_requires_explicit_experiment_and_exact_zero() {
        assert_eq!(max_api_version(VULKAN_1_1, true, Some("0")), 0);
        assert_eq!(max_api_version(VULKAN_1_1, false, Some("0")), VULKAN_1_1);
        for value in ["1", "", "false", "00", "0 ", "2"] {
            assert_eq!(max_api_version(VULKAN_1_1, true, Some(value)), VULKAN_1_1);
        }
    }

    #[test]
    fn rejects_each_effective_vulkan_1_0_before_skia() {
        assert!(supported(VULKAN_1_1, VULKAN_1_1, VULKAN_1_1));
        assert!(!supported(1 << 22, VULKAN_1_1, VULKAN_1_1));
        assert!(!supported(VULKAN_1_1, 1 << 22, VULKAN_1_1));
        assert!(!supported(VULKAN_1_1, VULKAN_1_1, 1 << 22));
    }
}
