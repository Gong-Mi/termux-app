//! 性能优化：ASCII 批量扫描 (SIMD/SVE)
//!
//! 提供高效的方法来跳过连续的可打印 ASCII 字符。

/// 快速扫描连续可打印 ASCII 字符 (0x20 - 0x7E) 的长度
#[inline(always)]
pub fn fast_skip_printable_len(data: &[u8]) -> usize {
    #[cfg(target_arch = "aarch64")]
    {
        // 在 aarch64 上尝试使用 SVE (Scalable Vector Extension)
        // 注意：这里需要运行时检测，因为并非所有 aarch64 芯片都支持 SVE
        if std::arch::is_aarch64_feature_detected!("sve") {
            return unsafe { sve_printable_scan(data) };
        }
    }

    // 回退到通用的优化路径 (SWAR)
    scalar_swar_scan(data)
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

    while i < len {
        let count: usize;
        let ptr_with_offset = unsafe { ptr.add(i) };

        // 使用 SVE 谓词操作批量查找
        unsafe {
            std::arch::asm!(
                "whilelo p0.b, xzr, {rem}",             // p0 = [1, 1, 1, 0, 0...] (针对数组边界)
                "ld1b {{z0.b}}, p0/z, [{base}]",        // z0 = data[i..i+VL]

                "cmphs p1.b, p0/z, z0.b, #32",          // p1 = z0 >= 0x20
                "cmpls p2.b, p0/z, z0.b, #126",         // p2 = z0 <= 0x7E
                "and p1.b, p0/z, p1.b, p2.b",           // p1 = (p1 & p2) 即满足 [32, 126]

                "not p2.b, p0/z, p1.b",                 // p2 = !p1 (查找非打印字符)
                "brkb p2.b, p0/z, p2.b",                // p2 = 1 for all elements BEFORE the first true in p2
                "and p1.b, p0/z, p1.b, p2.b",           // 结合边界和有效字符

                "cntp {count}, p0, p1.b",               // count = popcount(p1)
                rem = in(reg) (len - i),
                base = in(reg) ptr_with_offset,
                count = out(reg) count,
                out("z0") _, out("p0") _, out("p1") _, out("p2") _,
            );
        }

        if count == 0 {
            break;
        }
        i += count;

        // 获取当前 SVE 向量字节长度
        let vl: usize;
        unsafe {
            std::arch::asm!("cntb {0}", out(reg) vl);
        }
        if count < vl {
            break;
        } // 说明中间遇到了中断
    }

    i
}
