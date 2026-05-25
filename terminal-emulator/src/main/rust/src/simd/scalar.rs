use crate::pixel::Pixel8;

/// Simple scalar implementation of RGBA8 → RGBA10 conversion.
/// This is identical to the reference implementation used in existing tests.
#[inline]
pub fn convert_rgba8_to_rgba10_scalar(src: &[Pixel8], dst: &mut [u32]) {
    let len = src.len().min(dst.len());
    for i in 0..len {
        let p = src[i];
        let r10 = ((p.r as u32) * 1023 + 127) / 255;
        let g10 = ((p.g as u32) * 1023 + 127) / 255;
        let b10 = ((p.b as u32) * 1023 + 127) / 255;
        let a2 = (p.a as u32) >> 6;
        dst[i] = r10 | (g10 << 10) | (b10 << 20) | (a2 << 30);
    }
}
