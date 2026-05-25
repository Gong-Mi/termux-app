// src/pixel.rs

/// Simple representation of an 8-bit RGBA pixel used by the SIMD conversion utilities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pixel8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
