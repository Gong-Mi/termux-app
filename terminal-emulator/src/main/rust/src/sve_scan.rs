//! 性能优化：ASCII 批量扫描 (SIMD/SVE)
//!
//! 提供高效的方法来跳过连续的可打印 ASCII 字符。

/// 快速扫描连续可打印 ASCII 字符 (0x20 - 0x7E) 的长度。
///
/// 移动端 ARM 上按 128-bit SVE 做性能规划：这里利用的是 SVE 的
/// predicate/break/cntp 快速推进能力，而不是假设向量宽度大于 NEON。
///
/// 阈值选择参考 bench_sve_scan 实机数据：
///   K70U (D9300 X4):     SVE 在 <2KB 比 SWAR 慢 (asan_1KB 0.836x)
///                         2KB 之后开始正收益，1MB 达 1.122x
///   17 Pro (8e5 Oryon):  SVE 在 64B 就赢 (1.036x), 1MB 达 1.326x
///   综合阈值: 2048 (2KB), 覆盖 Cortex X4 的冷启动亏损区间,
///               Oryon 上 SWAR 处理 <2KB 也只需 ~0.1ns/B, 可忽略。
#[inline(always)]
pub fn fast_skip_printable_len(data: &[u8]) -> usize {
    // 短片段用 SWAR，避免 SVE predicate/asm 入口成本吞掉收益。
    // 阈值 2048 基于 D9300/8e5 实机 benchmark 校准。
    if data.len() < 2048 {
        return scalar_swar_scan(data);
    }

    #[cfg(target_arch = "aarch64")]
    {
        // 在 aarch64 上尝试使用 SVE (Scalable Vector Extension)
        // 注意：这里需要运行时检测，因为并非所有 aarch64 芯片都支持 SVE。
        if std::arch::is_aarch64_feature_detected!("sve") {
            return unsafe { sve_printable_scan(data) };
        }
    }

    // 回退到通用的优化路径 (SWAR)
    scalar_swar_scan(data)
}

/// Benchmark/test-visible scalar reference. Do not use as terminal semantics outside scan tests.
pub fn fast_skip_printable_len_scalar_reference(data: &[u8]) -> usize {
    scalar_swar_scan(data)
}

/// Runtime SVE vector length in bytes. Returns 0 when SVE is unavailable/non-aarch64.
pub fn sve_vector_len_bytes() -> usize {
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("sve") {
            return unsafe { sve_vector_len_bytes_sve() };
        }
    }
    0
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "sve")]
unsafe fn sve_vector_len_bytes_sve() -> usize {
    let vl: usize;
    unsafe {
        std::arch::asm!("cntb {0}", out(reg) vl);
    }
    vl
}

#[cfg(target_arch = "aarch64")]
pub unsafe fn fast_skip_printable_len_sve_unchecked_for_bench(data: &[u8]) -> usize {
    unsafe { sve_printable_scan(data) }
}

#[cfg(all(test, target_arch = "aarch64"))]
fn sve_vector_len_bytes_for_test() -> usize {
    sve_vector_len_bytes()
}

/// 基于 SWAR (SIMD Within A Register) 的标量优化路径
/// 一次处理 8 个字节
fn scalar_swar_scan(data: &[u8]) -> usize {
    let mut i = 0;
    let len = data.len();

    // 1. 处理未对齐的头部
    while i < len && (data.as_ptr() as usize + i) % 8 != 0 {
        let b = data[i];
        if b < 0x20 || b > 0x7E {
            return i;
        }
        i += 1;
    }

    // 2. 批量处理 8 字节块
    let chunks = (len - i) / 8;
    if chunks > 0 {
        let ptr = unsafe { data.as_ptr().add(i) as *const u64 };
        for j in 0..chunks {
            let word = unsafe { ptr.add(j).read_unaligned() };

            // 检查是否有任何字节不在 [0x20, 0x7E] 范围内
            // 可打印字符范围：0x20 (' ') 到 0x7E ('~')
            // 我们需要检测：
            // - 是否有字节 < 0x20
            // - 是否有字节 > 0x7E (即 c >= 0x7F)

            // 检测 c < 0x20: (word - 0x2020...) & 借位标志
            let low_check = (word.wrapping_sub(0x2020202020202020)) & 0x8080808080808080;
            // 检测 c > 0x7E (即 c >= 0x7F)
            // 简单点：是否有位 7 被设置 (c >= 0x80)
            let high_bit_check = word & 0x8080808080808080;
            // 检查 0x7F (DEL): (word ^ 0x7F...) == 0
            let del_xor = word ^ 0x7F7F7F7F7F7F7F7F;
            let has_del =
                ((del_xor.wrapping_sub(0x0101010101010101)) & !del_xor & 0x8080808080808080) != 0;

            if high_bit_check != 0 || low_check != 0 || has_del {
                break;
            }
            i += 8;
        }
    }

    // 3. 处理尾部字节
    while i < len {
        let b = data[i];
        if b < 0x20 || b > 0x7E {
            break;
        }
        i += 1;
    }
    i
}

/// SVE 汇编优化路径 (aarch64)
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "sve")]
unsafe fn sve_printable_scan(data: &[u8]) -> usize {
    let mut i: usize = 0;
    let len = data.len();
    let ptr = data.as_ptr();
    let vl: usize;

    unsafe {
        std::arch::asm!("cntb {0}", out(reg) vl);
    }

    while i < len {
        let rem = len - i;
        let ptr_with_offset = unsafe { ptr.add(i) };

        if rem >= vl * 4 {
            let c0: usize;
            let c1: usize;
            let c2: usize;
            let c3: usize;
            let p0 = ptr_with_offset;
            let p1 = unsafe { ptr_with_offset.add(vl) };
            let p2 = unsafe { ptr_with_offset.add(vl * 2) };
            let p3 = unsafe { ptr_with_offset.add(vl * 3) };

            unsafe {
                std::arch::asm!(
                    "ptrue p0.b",

                    "ld1b {{z0.b}}, p0/z, [{base0}]",
                    "cmphs p1.b, p0/z, z0.b, #32",
                    "cmpls p2.b, p0/z, z0.b, #126",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "not p2.b, p0/z, p1.b",
                    "brkb p2.b, p0/z, p2.b",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "cntp {c0}, p0, p1.b",

                    "ld1b {{z1.b}}, p0/z, [{base1}]",
                    "cmphs p1.b, p0/z, z1.b, #32",
                    "cmpls p2.b, p0/z, z1.b, #126",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "not p2.b, p0/z, p1.b",
                    "brkb p2.b, p0/z, p2.b",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "cntp {c1}, p0, p1.b",

                    "ld1b {{z2.b}}, p0/z, [{base2}]",
                    "cmphs p1.b, p0/z, z2.b, #32",
                    "cmpls p2.b, p0/z, z2.b, #126",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "not p2.b, p0/z, p1.b",
                    "brkb p2.b, p0/z, p2.b",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "cntp {c2}, p0, p1.b",

                    "ld1b {{z3.b}}, p0/z, [{base3}]",
                    "cmphs p1.b, p0/z, z3.b, #32",
                    "cmpls p2.b, p0/z, z3.b, #126",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "not p2.b, p0/z, p1.b",
                    "brkb p2.b, p0/z, p2.b",
                    "and p1.b, p0/z, p1.b, p2.b",
                    "cntp {c3}, p0, p1.b",

                    base0 = in(reg) p0,
                    base1 = in(reg) p1,
                    base2 = in(reg) p2,
                    base3 = in(reg) p3,
                    c0 = out(reg) c0,
                    c1 = out(reg) c1,
                    c2 = out(reg) c2,
                    c3 = out(reg) c3,
                    out("z0") _, out("z1") _, out("z2") _, out("z3") _,
                    out("p0") _, out("p1") _, out("p2") _,
                );
            }

            if c0 < vl {
                i += c0;
                break;
            }
            if c1 < vl {
                i += vl + c1;
                break;
            }
            if c2 < vl {
                i += vl * 2 + c2;
                break;
            }
            if c3 < vl {
                i += vl * 3 + c3;
                break;
            }
            i += vl * 4;
            continue;
        }

        let count: usize;

        // 使用 SVE 谓词操作批量查找。
        // 移动端按 VL=128-bit/16-byte 规划：收益来自 predicate fast-advance。
        // 完整 VL 用 ptrue，只有尾部才 whilelo，避免热循环重复边界谓词生成。
        if rem >= vl {
            unsafe {
                std::arch::asm!(
                    "ptrue p0.b",                         // full active vector
                    "ld1b {{z0.b}}, p0/z, [{base}]",      // z0 = data[i..i+VL]

                    "cmphs p1.b, p0/z, z0.b, #32",        // p1 = z0 >= 0x20
                    "cmpls p2.b, p0/z, z0.b, #126",       // p2 = z0 <= 0x7E
                    "and p1.b, p0/z, p1.b, p2.b",         // p1 = printable

                    "not p2.b, p0/z, p1.b",               // p2 = !printable
                    "brkb p2.b, p0/z, p2.b",              // lanes before first failure
                    "and p1.b, p0/z, p1.b, p2.b",

                    "cntp {count}, p0, p1.b",
                    base = in(reg) ptr_with_offset,
                    count = out(reg) count,
                    out("z0") _, out("p0") _, out("p1") _, out("p2") _,
                );
            }
        } else {
            unsafe {
                std::arch::asm!(
                    "whilelo p0.b, xzr, {rem}",
                    "ld1b {{z0.b}}, p0/z, [{base}]",

                    "cmphs p1.b, p0/z, z0.b, #32",
                    "cmpls p2.b, p0/z, z0.b, #126",
                    "and p1.b, p0/z, p1.b, p2.b",

                    "not p2.b, p0/z, p1.b",
                    "brkb p2.b, p0/z, p2.b",
                    "and p1.b, p0/z, p1.b, p2.b",

                    "cntp {count}, p0, p1.b",
                    rem = in(reg) rem,
                    base = in(reg) ptr_with_offset,
                    count = out(reg) count,
                    out("z0") _, out("p0") _, out("p1") _, out("p2") _,
                );
            }
        }

        if count == 0 {
            break;
        }
        i += count;

        if count < vl {
            break;
        } // 说明中间遇到了中断或尾部不足一个 VL
    }

    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deterministic_bytes(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            out.push((x >> 32) as u8);
        }
        out
    }

    fn cases() -> Vec<Vec<u8>> {
        let mut cases = vec![
            b"".to_vec(),
            b"a".to_vec(),
            b"hello world".to_vec(),
            b"\x1b[31mred".to_vec(),
            b"abc\x1b[31m".to_vec(),
            b"abc\ndef".to_vec(),
            b"abc\rdef".to_vec(),
            b"abc\x7fdef".to_vec(),
            "abc中文def".as_bytes().to_vec(),
        ];

        for len in 0..256 {
            let mut ascii = vec![b'x'; len];
            cases.push(ascii.clone());
            if len > 0 {
                ascii[len / 2] = b'\n';
                cases.push(ascii);
            }
            cases.push(deterministic_bytes(len, len as u64 + 0x5eed));
        }
        cases
    }

    #[test]
    fn scalar_swar_matches_naive_printable_scan() {
        for data in cases() {
            for offset in 0..=data.len().min(17) {
                let slice = &data[offset..];
                let expected = slice
                    .iter()
                    .position(|&b| !(0x20..=0x7e).contains(&b))
                    .unwrap_or(slice.len());
                assert_eq!(
                    scalar_swar_scan(slice),
                    expected,
                    "data={data:?} offset={offset}"
                );
            }
        }
    }

    #[test]
    #[cfg(target_arch = "aarch64")]
    fn sve_scan_matches_scalar_swar_when_available() {
        if !std::arch::is_aarch64_feature_detected!("sve") {
            return;
        }
        assert!(sve_vector_len_bytes_for_test() >= 16);
        for data in cases() {
            for offset in 0..=data.len().min(17) {
                let slice = &data[offset..];
                let scalar = scalar_swar_scan(slice);
                let sve = unsafe { sve_printable_scan(slice) };
                assert_eq!(sve, scalar, "data={data:?} offset={offset}");
            }
        }
    }
}
