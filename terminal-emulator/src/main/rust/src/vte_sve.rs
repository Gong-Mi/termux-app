#[cfg(target_arch = "aarch64")]
use std::arch::asm;

/// 使用 SVE 加速寻找第一个控制字符的位置
/// 返回纯文本的长度。
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "sve")]
pub unsafe fn find_first_control_sve(data: &[u8]) -> usize {
    let mut ptr = data.as_ptr();
    let end = unsafe { ptr.add(data.len()) };
    let start_ptr = ptr;

    // SVE 的向量长度是不定的 (Scalable)
    while ptr < end {
        let mut first_ctrl_idx: u64;

        unsafe {
            asm!(
                "ptrue p0.b",                          // 设置全通谓词
                "whilelt p0.b, {ptr}, {end}",          // 根据剩余长度生成谓词
                "ld1b {{z0.b}}, p0/z, [{ptr}]",        // 谓词加载数据到 z0

                // 检查控制字符 (byte < 32)
                "mov z1.b, #31",                       // 将 31 放入 z1
                "cmphs p1.b, p0/z, z1.b, z0.b",        // p1 = (31 >= byte) 即 (byte <= 31)

                // 检查 DEL (127)
                "mov z2.b, #127",                      // 将 127 放入 z2
                "cmpeq p2.b, p0/z, z0.b, z2.b",        // p2 = (byte == 127)

                "orrs p1.b, p0/z, p1.b, p2.b",         // p1 = (byte <= 31) || (byte == 127)
                "brkb p1.b, p0/z, p1.b",               // 找到第一个匹配位之前的连续位
                "cntp {idx}, p0, p1.b",                // 统计前半部分纯文本长度

                ptr = in(reg) ptr,
                end = in(reg) end,
                idx = out(reg) first_ctrl_idx,
                out("p0") _, out("p1") _, out("p2") _, out("z0") _, out("z1") _, out("z2") _
            );
        }

        let len_processed: u64;
        unsafe {
            asm!("cntp {lp}, p0, p0.b", lp = out(reg) len_processed);
        }

        if first_ctrl_idx < len_processed {
            // 找到了控制字符，返回总偏移
            return (ptr as usize - start_ptr as usize) + (first_ctrl_idx as usize);
        }

        // 全是纯文本，前进向量步长
        let vec_bytes: u64;
        unsafe {
            asm!("cntb {v}", v = out(reg) vec_bytes);
            ptr = ptr.add(vec_bytes as usize);
        }
    }

    data.len()
}

/// 运行时检测是否支持 SVE
pub fn has_sve_support() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        // 在 Android 上使用 std::arch 进行探测
        std::arch::is_aarch64_feature_detected!("sve")
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}
