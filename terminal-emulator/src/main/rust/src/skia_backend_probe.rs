//! CI experiment only: a Ganesh Vulkan render target, never a raster fallback.
use skia_safe::{AlphaType, Color, ColorType, ImageInfo, Paint, Rect, gpu};

pub(crate) fn draw_and_readback(context: &mut gpu::DirectContext) -> Result<(), &'static str> {
    if context.backend() != gpu::BackendAPI::Vulkan {
        return Err("not a Vulkan context");
    }
    let info = ImageInfo::new((8, 8), ColorType::RGBA8888, AlphaType::Premul, None);
    let mut surface = gpu::surfaces::render_target(
        context,
        gpu::Budgeted::Yes,
        &info,
        None,
        gpu::SurfaceOrigin::TopLeft,
        None,
        false,
        false,
    )
    .ok_or("GPU render target allocation failed")?;
    surface.canvas().clear(Color::BLUE);
    let mut paint = Paint::default();
    paint.set_color(Color::RED);
    paint.set_anti_alias(false);
    surface
        .canvas()
        .draw_rect(Rect::from_xywh(2.0, 2.0, 4.0, 4.0), &paint);
    context.flush_and_submit();
    let mut pixels = [0u8; 8 * 8 * 4];
    // Ganesh readPixels synchronously transfers the GPU result to CPU memory.
    if !surface.read_pixels(&info, &mut pixels, 8 * 4, (0, 0)) {
        return Err("GPU read_pixels failed");
    }
    verify_pixels(&pixels)
}

fn verify_pixels(pixels: &[u8; 8 * 8 * 4]) -> Result<(), &'static str> {
    for y in 0..8 {
        for x in 0..8 {
            let expected = if (2..6).contains(&x) && (2..6).contains(&y) {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
            let offset = (y * 8 + x) * 4;
            if pixels[offset..offset + 4] != expected {
                return Err("GPU pixel mismatch");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pixel-oracle unit test only; this does not claim to execute a GPU draw.
    #[test]
    fn oracle_rejects_blank_clear_only_and_wrong_draw() {
        assert!(verify_pixels(&[0; 256]).is_err());
        let mut pixels = [0u8; 256];
        for y in 0..8 {
            for x in 0..8 {
                let offset = (y * 8 + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        assert!(verify_pixels(&pixels).is_err());
        for y in 2..6 {
            for x in 2..6 {
                let offset = (y * 8 + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
            }
        }
        assert_eq!(verify_pixels(&pixels), Ok(()));
        pixels[0] = 1;
        assert!(verify_pixels(&pixels).is_err());
    }
}
