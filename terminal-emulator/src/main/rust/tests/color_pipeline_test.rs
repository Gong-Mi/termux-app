#[cfg(test)]
mod tests {
    use ash::vk;
    use skia_safe::ColorSpace;

    /// 模拟渲染器内部的颜色空间映射逻辑，用于验证逻辑正确性
    fn map_vk_color_space_to_skia(vk_color_space: vk::ColorSpaceKHR) -> Option<ColorSpace> {
        match vk_color_space {
            vk::ColorSpaceKHR::DISPLAY_P3_NONLINEAR_EXT => ColorSpace::new_cicp(
                skia_safe::named_primaries::CicpId::SMPTE_EG_432_1,
                skia_safe::named_transfer_fn::CicpId::SRGB,
            ),
            vk::ColorSpaceKHR::HDR10_ST2084_EXT => ColorSpace::new_cicp(
                skia_safe::named_primaries::CicpId::Rec2020,
                skia_safe::named_transfer_fn::CicpId::PQ,
            ),
            vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT => Some(ColorSpace::new_srgb_linear()),
            vk::ColorSpaceKHR::SRGB_NONLINEAR => Some(ColorSpace::new_srgb()),
            _ => None,
        }
    }

    #[test]
    fn test_color_space_mapping() {
        // 1. 验证 sRGB 映射
        let srgb = map_vk_color_space_to_skia(vk::ColorSpaceKHR::SRGB_NONLINEAR).unwrap();
        assert!(srgb.is_srgb());

        // 2. 验证 Display P3 映射
        let p3 = map_vk_color_space_to_skia(vk::ColorSpaceKHR::DISPLAY_P3_NONLINEAR_EXT).unwrap();
        assert!(!p3.is_srgb());

        // 3. 验证 HDR10 (PQ) 映射
        let hdr10 = map_vk_color_space_to_skia(vk::ColorSpaceKHR::HDR10_ST2084_EXT).unwrap();
        assert!(!hdr10.is_srgb());

        // 4. 验证 scRGB (Linear) 映射
        let scrgb =
            map_vk_color_space_to_skia(vk::ColorSpaceKHR::EXTENDED_SRGB_LINEAR_EXT).unwrap();
        // 线性伽马
        assert!(!scrgb.is_srgb());
    }

    #[test]
    fn test_format_priority_logic() {
        // 模拟 surface_formats 列表
        let formats = vec![
            vk::SurfaceFormatKHR {
                format: vk::Format::R8G8B8A8_UNORM,
                color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
            },
            vk::SurfaceFormatKHR {
                format: vk::Format::A2B10G10R10_UNORM_PACK32,
                color_space: vk::ColorSpaceKHR::DISPLAY_P3_NONLINEAR_EXT,
            },
            vk::SurfaceFormatKHR {
                format: vk::Format::A2B10G10R10_UNORM_PACK32,
                color_space: vk::ColorSpaceKHR::HDR10_ST2084_EXT,
            },
        ];

        // 模拟我们的优先级选择算法
        let selected = formats
            .iter()
            .find(|f| {
                f.format == vk::Format::A2B10G10R10_UNORM_PACK32
                    && f.color_space == vk::ColorSpaceKHR::HDR10_ST2084_EXT
            })
            .or_else(|| {
                formats.iter().find(|f| {
                    f.format == vk::Format::A2B10G10R10_UNORM_PACK32
                        && f.color_space == vk::ColorSpaceKHR::DISPLAY_P3_NONLINEAR_EXT
                })
            })
            .or_else(|| {
                formats
                    .iter()
                    .find(|f| f.format == vk::Format::A2B10G10R10_UNORM_PACK32)
            })
            .copied()
            .unwrap();

        // 验证 HDR10 优先级最高
        assert_eq!(selected.color_space, vk::ColorSpaceKHR::HDR10_ST2084_EXT);
        assert_eq!(selected.format, vk::Format::A2B10G10R10_UNORM_PACK32);
    }
}
