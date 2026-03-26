# 需要大规模重构的功能分析

**分析日期**: 2026-03-26

---

## 当前状态更新

经过详细代码审查，发现以下**高优先级功能已实现**：

### ✅ 已实现的高优先级功能

| 功能 | 实现位置 | 状态 |
|------|----------|------|
| **resize 快速路径** | `screen.rs::resize_rows_only()` (line 513-579) | ✅ 完整实现 O(1) |
| **active_transcript_rows 增量维护** | `screen.rs::scroll_up()` (line 250-253) | ✅ 完整实现 |
| **first_row 计算逻辑** | `screen.rs::resize_rows_only()` (line 554-560) | ✅ 对齐 Java |
| **空行跳过逻辑** | `screen.rs::resize_with_reflow()` (line 336-362) | ✅ 实现 Java 逻辑 |

---

## 真正需要大规模重构的功能

### 🔴 1. getScreen() 兼容层 - 需要架构级重构

**问题**: 
- Rust 使用共享内存直接访问屏幕数据
- Java 应用层可能依赖 `TerminalBuffer.getScreen()` 方法
- 当前返回 `null`，可能破坏依赖此方法的应用

**重构需求**:
1. 创建 `TerminalBufferCompat.java` 包装类
2. 实现与官方 `TerminalBuffer` 兼容的 API
3. 通过 JNI 从 Rust 共享内存读取数据
4. 维护额外的数据结构（增加内存开销）

**预计工作量**: 2-3 天  
**风险**: 高（可能引入新的 bug）  
**建议**: 低优先级，除非有应用明确依赖

---

### 🟡 2. DCS/APC 完整处理 - 需要扩展解析器

**问题**:
- 当前 DCS/APC 只有框架，具体命令处理不完整
- Sixel 以外的 DCS 功能缺失

**重构需求**:
1. 扩展 `vte_parser.rs` 状态机
2. 添加更多 DCS 命令处理器
3. 实现 APC 命令解析
4. 添加 PM/SOS 支持

**预计工作量**: 2-3 天  
**风险**: 中  
**建议**: 中优先级，按需实现

---

### 🟡 3. 边界条件完全对齐 - 需要仔细验证

**问题**:
- 换行符样式检查缺失
- null 行检查缺失
- 光标处理边界过于激进

**重构需求**:
1. 在 `resize_with_reflow()` 中添加样式检查
2. 添加 null 行检查（虽然 Rust 用 Vec 不会出现 null）
3. 调整光标放置逻辑

**预计工作量**: 0.5-1 天  
**风险**: 低  
**建议**: **高优先级，立即修复**

---

## 不需要重构的功能（已实现）

### ✅ 已完整实现

| 功能 | 状态 | 备注 |
|------|------|------|
| resize 快速路径 | ✅ | `resize_rows_only()` O(1) 实现 |
| active_transcript_rows 增量维护 | ✅ | `scroll_up()` 中实现 |
| first_row 计算 | ✅ | 与 Java 公式一致 |
| 空行跳过逻辑 | ✅ | 使用 `skipped_blank_lines` |
| DECSET 统一处理 | ✅ | `do_decset_or_reset()` 调用 `handle_decset()` |
| processCodePoint | ✅ | JNI 接口已实现 |
| toString | ✅ | `getDebugInfoFromRust()` 已实现 |
| setCursorStyle | ✅ | JNI 接口已实现 |
| 鼠标常量 | ✅ | 已定义 |

---

## 修复优先级更新

### 立即修复（0.5-1 天）

1. **边界条件对齐**
   - 添加换行符样式检查
   - 调整光标放置逻辑
   - 文件：`screen.rs::resize_with_reflow()`

### 短期修复（2-3 天）

2. **DCS/APC 扩展**
   - 补充具体命令处理
   - 文件：`vte_parser.rs`, `terminal/handlers/`

### 长期考虑（按需）

3. **getScreen() 兼容层**
   - 仅在应用明确需要时实现
   - 文件：新文件 `TerminalBufferCompat.java`

---

## 结论

**好消息**: 经过审查，发现**大部分高优先级功能已经实现**！

**需要重构的功能**:
1. **边界条件对齐** - 小改动，0.5-1 天
2. **DCS/APC 扩展** - 中等改动，2-3 天
3. **getScreen() 兼容层** - 大规模重构，2-3 天（可选）

**建议**: 
- 立即修复边界条件（高优先级）
- DCS/APC 按需扩展
- getScreen() 兼容层仅在必要时实现
