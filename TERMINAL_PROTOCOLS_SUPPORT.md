# Termux 增强终端协议支持矩阵

目前 Rust 引擎已实现的现代终端协议汇总。

## 1. 键盘协议 (Input)
- **Kitty Keyboard Protocol**: ✅ 已实现
  - 支持 `CSI > 1 u` 开启。
  - 精准区分 `Shift+Space`, `Ctrl+I` 与 `Tab` 等组合键。
  - Gemini CLI 强依赖。
- **xterm modifyOtherKeys**: ✅ 已实现
  - 支持 `CSI > 4 ; 2 m`。

## 2. 鼠标与粘贴
- **SGR Mouse Tracking**: ✅ 已实现 (`CSI ? 1006 h`)
- **Bracketed Paste**: ✅ 已实现 (`CSI ? 2004 h`)

## 3. 渲染与特殊元素
- **TrueColor (24-bit RGB)**: ✅ 已实现
- **Block Elements (U+2580 - U+259F)**: ✅ 完美支持 (已修复覆盖泄露 Bug)
- **Powerline / Round Corners**: ✅ 完美支持 (已修复位移残影 Bug)
- **Sixel Graphics**: ✅ 已实现 (带 GPU 纹理缓存)

## 4. 待办协议 (Roadmap)
- **HDR10 (10-bit color)**: ⏳ 规划中 (涉及 128-bit TextStyle 重构)
- **Kitty Image Protocol**: ❌ 未实现
