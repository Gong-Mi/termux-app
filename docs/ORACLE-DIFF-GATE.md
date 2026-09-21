# 差分 oracle 门禁（Rust 引擎 vs 上游参考实现）

日期：2026-09-22
分支：`test/oracle-diff-gate`
工作流作业：`.github/workflows/compare-engines.yml` → `functional-oracle`

## 问题

这个仓库的测试期望值全部由编写引擎的人手写。consistency.rs 里是
`assert_eq!(engine.state.cursor.x, 11, "Cursor X should be 11")`，calibration_*.db 是自采数据。
这类测试能抓住"我写错了"，抓不住"我以为对的理解本身就错了"——因为盲点和期望值是同一个
人写的。对"没有发现问题直觉"的情形，手写期望值不是证据，只是把未知重写了一遍。

所以缺的不是更多测试，而是**外部参照**。

## 机制

同一份字节语料喂给两个独立实现，逐列比对最终状态：

```
corpus/sequences/seed.jsonl          86 条序列 / 161 步（文本、光标、擦除、编辑、边距、
        |                            备用屏、OSC、DCS、字符集、宽字符、真实程序输出）
        |
        +-- tools/oracle/run.sh --> 上游参考实现（TerminalEmulator.java）
        |        |                  pinned: e634d8f981f48b6b89202cf0e04533f0889e03b3
        |        v                 （v0.119.0-beta.3，本仓库基线）
        |    corpus/golden/upstream-e634d8f.jsonl   ← 期望值（不是本仓库作者写的）
        |
        +-- cargo test --test oracle_diff --> Rust 引擎
                 |
                 v
           target/oracle_diff_report.json     ← 差异清单（哪条序列/哪一步/哪一列/期望 vs 实际）
```

比较的层次是**列空间**：每一列实际落在哪个码点、宽字符的第二列是否作为续列、该列的样式位。
这是两个实现必须一致的层次，因为渲染器画的就是它、换行也是按它算的。上游把宽字符压进
一个数组槽（紧凑存储），本引擎按列存储，所以直接比数组会得到表示差异而不是行为差异。

## 现在能证明什么

- 参考侧在普通 JVM 上跑通，不需要 Android SDK（`tools/oracle/build.sh` 只编译纯 Java 子集 +
  3 个 Android stub + 真实平台键值）。本机实跑：86 序列 / 161 快照 / 303,260 列。
- 黄金文件可由固定 commit 复现：CI 每次重新生成并与提交的黄金文件逐字节比对，不一致即失败
  （防止参考实现或 harness 悄悄漂移）。
- 覆盖率守卫：语料里每条序列都必须出现在黄金文件中，比较列数必须超过下限；否则门禁直接红。
  "几乎什么都没比"的绿色比没有门禁更糟。

## 现在不能证明什么（边界）

- **不是真机/GPU 证据**：这里只比终端状态语义，不涉及渲染、驱动、Android 运行时。
  渲染线与 API 线各有自己的门禁。
- **组合字符（零宽）只统计数量**：列空间比较不覆盖零宽字符的组合位置，按数量记录为软差异。
- **事件与响应是软指标**：transcript 行数/首行/transcript 摘要、客户端写入次数记录在报告里，
  默认不判定失败（它们依赖缓冲区布局，属于"可能分叉但需先确认语义"的一组）。
- **不注入按键**：语料是 PTY 字节流，不是 KeyEvent；按键路径（KeyHandler 的 termcap 映射）
  只保证能编译、不参与比较。
- 本机没有 rustfmt/clippy，Rust 侧格式与 lint 未在本地验证（CI 的 Rust CI 作业会跑）。

## 棘轮（ratchet）

移植不会一次对齐，所以已知差异记在 `corpus/oracle_baseline.json`（id → 差异列数）：

- 新增差异、或已有差异变大 → 门禁失败；
- 差异变小 → 打印"baseline can shrink"，提示更新；
- 确认要接受当前状态时：`ORACLE_WRITE_BASELINE=1 cargo test --test oracle_diff`。

环境变量：`ORACLE_MODE=report|gate`、`ORACLE_GOLDEN`、`ORACLE_CORPUS`、`ORACLE_BASELINE`、
`ORACLE_DIFF_REPORT`、`ORACLE_MAX_DIFFS_SHOWN`。

## 纪律

1. 真机或 CI 上发现的任何终端语义问题，当天加一条语料（`corpus/sequences/case-<名字>.jsonl`
   或被引用的 seed 生成器条目），让它在门禁里永久可复现。
2. 改引擎语义前先跑门禁：如果没有任何门禁会因这次改动变红或变绿，先补门禁再改代码。
3. 黄金文件只由 `tools/oracle/run.sh` 生成，不手工编辑。

## 怎么跑

```bash
# 本机（需要 javac 21；参考树在 pinned commit 上）
git clone https://github.com/termux/termux-app ~/termux-src/termux-app
git -C ~/termux-src/termux-app checkout e634d8f981f48b6b89202cf0e04533f0889e03b3
bash tools/oracle/run.sh                       # 重新生成黄金文件
cd terminal-emulator/src/main/rust && cargo test --test oracle_diff -- --nocapture

# CI
gh workflow run compare-engines.yml --ref <branch>
```