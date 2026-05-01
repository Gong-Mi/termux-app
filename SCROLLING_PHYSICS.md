# Termux-App Scrolling Physics Specification

This document details the mathematical models and implementation strategies used to achieve "Perceptual Smoothness" and "Sensory Damping" in the Rust-integrated Termux terminal emulator.

## 1. Problem Statement
Standard scrolling algorithms often suffer from "The Jittery Linear Trap":
- **Linear Mapping**: Scrolling speed is directly proportional to finger velocity. At high speeds, this leads to uncontrollable jumps ("彈射" effect) where a short flick covers thousands of rows.
- **Row-Based Discretization**: Calculating offsets in integer row counts leads to "stepping" visual artifacts, lacking the smoothness of modern fluid UIs.

## 2. Implemented Algorithms

### 2.1 Logarithmic Input Mapping (Sensory Damping)
To provide a natural "resistance" that increases with speed (similar to how humans perceive brightness or sound), we apply a logarithmic compression to the input distance $d$ and velocity $v$.

**The Model:**
$$d_{out} = \text{sign}(d_{in}) \cdot K \cdot \ln(1 + \frac{|d_{in}|}{K})$$

- **Low Velocity ($|d_{in}| \ll K$):** The behavior is nearly linear ($\ln(1+x) \approx x$), ensuring precise character-level control.
- **High Velocity ($|d_{in}| \gg K$):** The output grows logarithmically, effectively "braking" violent gestures and preventing the terminal from jumping to the end of the history.
- **Implementation Constant ($K$):** Tuned to `20.0f` for touch scrolling and `1500.0f` for flick gestures.

### 2.2 Quadratic Drag & Pixel-Space Integration
Unlike the Google Play version which operates in "Row Space," this implementation operates in **Pixel Space**.

1. **Pixel Accumulation**: We track `mFineScrollY` (float) to store sub-pixel and sub-row offsets.
2. **Translation Matrix**: The Rust renderer uses `canvas.translate(0, -(mFineScrollY % fontHeight))` to provide sub-row smooth scrolling.
3. **Friction Model**: We use `OverScroller` with a custom friction coefficient ($1.5 \times$ system default) to simulate physical mass.

## 3. Comparison with Industry Standards

| Feature | Standard (Google Play) | This Implementation | iOS (Reference) |
| :--- | :--- | :--- | :--- |
| **Damping Type** | Linear | **Logarithmic + Quadratic** | Viscous (Exponential) |
| **Coordinate System** | Row-based | **Pixel-based** | Pixel-based |
| **High-Speed Control** | None (Jumps to end) | **Self-Saturating** | Friction-limited |
| **Visual Artifacts** | Stepping/Jumping | **Fluid/Sub-row** | Fluid |

## 4. Verification & Testing Strategy

### 4.1 Manual Verification Scenarios
- **The "Gentle Slide"**: Move finger slowly. The text should track the finger precisely 1:1.
- **The "Violent Flick"**: Perform a very fast swipe. The list should accelerate smoothly but stop within a predictable range (approx. 2-3 screen heights), rather than hitting the bottom instantly.
- **The "IME Toggle"**: Open/close the keyboard while scrolling. The scroll position should remain stable, and the animation should not jitter.

### 4.2 Automated Logic Tests (Rust)
Run the following to verify the underlying math in the renderer:
```bash
cd terminal-emulator/src/main/rust
cargo test vulkan_context::tests
```

### 4.3 Logcat Monitoring
Filter for `TerminalView-Scroll` to observe real-time damping values:
```bash
adb logcat -s TerminalView-Scroll:D
```

---
*Date: May 1, 2026*
*Author: Gemini CLI*
