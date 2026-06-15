# Pinch Zoom / Long Scrollback Reflow Optimization Maps

Commit: `f34e3bc2 Defer terminal reflow during pinch zoom`

## 1. 技术图：缩放事件如何从“连续重排”变成“视觉缩放 + 一次提交”

### Before：旧路径，缩放过程中反复触发真实 resize/reflow

```mermaid
flowchart TD
    A[用户双指缩放 / Pinch] --> B[ScaleGestureDetector.onScale]
    B --> C[TerminalView.onScale]
    C --> D[TermuxTerminalViewClient.onScale]
    D --> E{scale < 0.9 或 > 1.1?}
    E -- 是 --> F[changeFontSize +/-2]
    F --> G[TerminalView.setTextSize]
    G --> H[nativeSetFontSize]
    G --> I[refreshFontMetrics]
    G --> J[updateSize]
    J --> K[session.updateSize]
    K --> L[RustTerminal.resize]
    L --> M[Screen.resize_with_reflow]
    M --> N{列数是否变化?}
    N -- 是 --> O[慢路径: 遍历 active transcript]
    O --> P[逐行逐字符重排 scrollback]
    P --> Q[ScreenUpdated + request_render]
    E -- 否 --> R[只保留 mScaleFactor]

    style O fill:#5b1b1b,stroke:#fb7185,color:#fff
    style P fill:#5b1b1b,stroke:#fb7185,color:#fff
    style Q fill:#1e293b,stroke:#94a3b8,color:#fff
```

问题点：

- `onScale` 是高频事件，一次手势里会触发很多次。
- 每次超过阈值就真实改变 font size。
- 真实 font size 改变后会重新计算 cols/rows。
- cols 变化会进入 `resize_with_reflow()` 慢路径。
- 长 scrollback 下复杂度接近：

```text
O(active_transcript_rows × old_columns)
```

也就是说，长日志/长对话越多，缩放时越容易卡。

---

### After：新路径，缩放中只视觉缩放，松手后一次真实提交

```mermaid
flowchart TD
    A[用户双指缩放 / Pinch] --> B[ScaleGestureDetector.onScale]
    B --> C[TerminalView.onScale]
    C --> D[累计 mScaleFactor]
    D --> E[TermuxTerminalViewClient.onScale]
    E --> F[返回 scale，不改 font size]
    F --> G[updateRenderParamsToRust]
    G --> H[nativeUpdateRenderParams scale]
    H --> I[render_thread::request_render]
    I --> J[TerminalRenderer.draw_frame 按 scale 视觉绘制]

    A --> K[手指松开 / ScaleGestureDetector.onScaleEnd]
    K --> L[TerminalView.onScaleEnd]
    L --> M[TermuxTerminalViewClient.onScaleEnd]
    M --> N{累计 scale < 0.9 或 > 1.1?}
    N -- 否 --> O[保持视觉 scale]
    N -- 是 --> P[计算最终 font size]
    P --> Q[Preferences.setFontSize]
    Q --> R[TerminalView.setTextSize]
    R --> S[nativeSetFontSize]
    R --> T[updateSize]
    T --> U[RustTerminal.resize]
    U --> V[最多一次 resize_with_reflow]
    V --> W[ScreenUpdated + request_render]

    style F fill:#064e3b,stroke:#34d399,color:#fff
    style J fill:#083344,stroke:#22d3ee,color:#fff
    style V fill:#78350f,stroke:#fbbf24,color:#fff
```

新路径的关键变化：

- 缩放过程中：只更新 `mScaleFactor`，走 Rust 视觉缩放。
- 不调用 `changeFontSize()`。
- 不调用 `setTextSize()`。
- 不触发 `updateSize()`。
- 不触发 `resize_with_reflow()`。
- 手势结束后：如果累计 scale 达到阈值，才提交一次真实字号变化。

---

## 2. 技术分层图：哪些层负责什么

```mermaid
flowchart LR
    subgraph Touch[触摸输入层]
        A[GestureAndScaleRecognizer]
        A1[onScale]
        A2[onScaleEnd]
    end

    subgraph View[TerminalView 视图层]
        B[mScaleFactor]
        B1[updateRenderParamsToRust]
        B2[setTextSize]
        B3[updateSize]
    end

    subgraph App[Termux App 产品策略层]
        C[TermuxTerminalViewClient]
        C1[onScale: 不提交字号]
        C2[onScaleEnd: 提交最终字号]
        C3[TermuxPreferences.setFontSize]
    end

    subgraph Native[JNI / Rust 渲染层]
        D[nativeUpdateRenderParams]
        D1[nativeSetFontSize]
        D2[render_thread::request_render]
        D3[TerminalRenderer.draw_frame scale]
    end

    subgraph Emulator[Rust 终端状态层]
        E[RustTerminal.resize]
        E1[Screen.resize_rows_only 快路径]
        E2[Screen.resize_with_reflow 慢路径]
    end

    A --> A1 --> B --> C --> C1 --> B1 --> D --> D2 --> D3
    A --> A2 --> C2 --> C3 --> B2 --> D1
    B2 --> B3 --> E
    E --> E1
    E --> E2

    style C1 fill:#064e3b,stroke:#34d399,color:#fff
    style D3 fill:#083344,stroke:#22d3ee,color:#fff
    style E2 fill:#5b1b1b,stroke:#fb7185,color:#fff
```

职责边界：

| 层 | 职责 | 本次变化 |
|---|---|---|
| GestureAndScaleRecognizer | 识别缩放手势 | 新增 `onScaleEnd` |
| TerminalView | 保存临时视觉 scale，通知 Rust 绘制 | 缩放中只传 scale，不提交字号 |
| TermuxTerminalViewClient | 决定什么时候真正改变字号 | 从 `onScale` 移到 `onScaleEnd` |
| Rust render_thread | 按 scale 绘制当前帧 | 继续使用已有 `nativeUpdateRenderParams` |
| Rust terminal state | resize/reflow 终端状态 | 从高频触发变成最多一次触发 |

---

## 3. 产品图：用户体验从“边捏边卡”变成“边捏边预览，松手确认”

### 用户视角流程

```mermaid
journey
    title Pinch Zoom User Journey
    section 旧体验
      长日志/长对话里开始双指缩放: 3: 用户
      缩放过程中不断真实改字号: 2: 系统
      scrollback 反复重排: 1: 系统
      画面卡顿/掉帧/跟手差: 1: 用户
      松手后才稳定: 3: 用户
    section 新体验
      长日志/长对话里开始双指缩放: 4: 用户
      缩放过程中只做视觉预览: 5: 系统
      画面跟手放大/缩小: 5: 用户
      松手后一次提交真实字号: 4: 系统
      最终 cols/rows 稳定: 5: 用户
```

---

### 产品状态机

```mermaid
stateDiagram-v2
    [*] --> Normal

    Normal --> VisualZooming: 双指缩放开始
    VisualZooming --> VisualZooming: onScale / 只更新视觉 scale
    VisualZooming --> CommitZoom: 手指松开 onScaleEnd

    CommitZoom --> Normal: scale 未超过阈值 / 不改字号
    CommitZoom --> Resizing: scale 超过阈值 / 提交 font size
    Resizing --> ReflowOnce: cols/rows 改变
    ReflowOnce --> Normal: 渲染稳定

    note right of VisualZooming
      只做预览
      不 resize
      不 reflow
      不写 preference
    end note

    note right of ReflowOnce
      只允许一次真实 reflow
      成本集中在松手后
    end note
```

---

## 4. 验收标准

### 日志验收

安装后执行：

```bash
logcat | grep -E "Pinch zoom commit|nativeSetFontSize|RustTerminal.resize|resize_with_reflow"
```

预期：

1. 双指缩放过程中，不应该连续刷 `nativeSetFontSize`。
2. 双指缩放过程中，不应该连续刷 resize/reflow 相关日志。
3. 松手后如果字号实际改变，只出现一次：

```text
Pinch zoom commit: scale=..., fontSize=old -> new
```

4. 如果缩放幅度小于阈值，不出现 commit 日志。

### 体验验收

| 场景 | 旧表现 | 新预期 |
|---|---|---|
| 普通短 scrollback 缩放 | 可接受 | 仍然可接受 |
| 几千行日志后缩放 | 可能卡顿 | 缩放中明显更跟手 |
| 几万行日志后缩放 | 容易明显掉帧 | 缩放中只预览，松手后一次短暂停顿可接受 |
| 快速连续 pinch | 多次 reflow | 每次手势最多一次 commit |
| 缩放幅度很小 | 可能累计触发 | 不提交字号，只保留视觉 scale |

---

## 5. 当前方案边界

这次优化解决的是：

```text
高频 onScale 事件反复触发真实 resize/reflow
```

它没有彻底消除：

```text
最终真实字号变化时，cols 改变仍需要一次 resize_with_reflow
```

如果后续还要继续优化，需要进入第二阶段：

1. viewport-first reflow：优先重排可见区域，历史区域延迟重排。
2. lazy scrollback reflow：滚到历史区域时再按需重排。
3. cache line-wrap segments：缓存旧列宽下的逻辑行分段，减少重复逐字符扫描。
4. reflow budget：每帧只处理固定数量历史行，避免单帧长阻塞。

但这些都属于更大架构改动；当前提交是低风险第一阶段。
