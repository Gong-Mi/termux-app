# Exec 链路验证记录

> 日期：2026-05-23
> 目标：区分 `gh/git` 失败到底是 exec/linker 问题、Go `os/exec` 问题，还是网络/TLS 问题。

## 当前判断

`gh repo clone` 不是单一问题，至少要拆成三段看：

1. `gh` 自己访问 GitHub API。
2. `gh` 通过 Go `os/exec` 启动 `git`。
3. `git` 访问 GitHub HTTPS 远端。

只有第二段失败时，才是 exec/linker/Go 补丁问题。

已观察到的错误分类：

| 错误 | 阶段 | 判断 |
|---|---|---|
| `failed to run git: fork/exec .../bin/git: permission denied` | `gh -> git` | exec/Go `os/exec` 没有走 linker |
| `Post "https://api.github.com/graphql": EOF` | `gh` API | 网络/TLS/API 连接提前断开 |
| `SSL: no alternative certificate subject name matches target hostname` | `git` HTTPS | 证书被代理/MITM 替换或 TLS 配置异常 |
| `TLS connect error ... unexpected eof while reading` | `git` HTTPS | 网络/TLS 连接被断开 |
| `Cloning into ...` 后继续 fetch/tag 输出 | `git` 已启动 | exec 已经通过，剩余看网络 |

## 不可信测试

不要用下面这种方式判断 Termux shell 环境：

```bash
adb shell input text 'echo%20PATH=$PATH'
```

原因：

- `%20` 不一定会变成空格，屏幕里可能实际执行 `echo%20PATH=...`。
- `$PATH` 可能被宿主 shell 或 `adb shell` 侧提前展开。
- 屏幕 scrollback 里可能混有更新 APK 前的旧输出。

也不要直接把 `run-as com.termux sh` 的默认环境当成 Termux 会话环境。它通常是系统环境，例如 `HOME=/`、系统 PATH，和 App 内 `bash -l` 不一致。

## 可信入口

更接近屏幕会话的入口是：

```bash
APPDIR="$(pwd)"
PREFIX="$APPDIR/files/usr"
HOME="$APPDIR/files/home"
TMPDIR="$PREFIX/tmp"
PATH="$PREFIX/bin:/system/bin"
LD_PRELOAD="$PREFIX/lib/libtermux-exec-ld-preload.so"
export PREFIX HOME TMPDIR PATH LD_PRELOAD

exec /system/bin/linker64 "$PREFIX/bin/bash" -lc '...'
```

用法：

```bash
adb push scripts/tmp-screenlike-gh-test.sh /data/local/tmp/screenlike-gh-test.sh
adb shell chmod 755 /data/local/tmp/screenlike-gh-test.sh
adb shell run-as com.termux sh /data/local/tmp/screenlike-gh-test.sh
```

这个方式避免宿主 shell 提前展开变量，也能模拟 App 里 `linker64 bash -l` 的执行方式。

## 基础回归

长期回归入口：

```bash
bash scripts/adb-termux-exec-probe.sh
```

需要关注：

- `failed_required=0`
- `git local clone` PASS
- `python subprocess` PASS
- `node child_process` PASS
- `gh executable` PASS
- `clang self-location` PASS

`go os/exec` 目前是 optional。设备未安装 Go 时会 XFAIL：

```text
spawn failed: No such file or directory
```

这不代表 exec 链路失败。

## APK 与 wrapper 检查

确认安装包里没有全局 wrapper：

```bash
APK="$(find termux-app/build/outputs/apk/debug -name '*.apk' | head -1)"
unzip -l "$APK" | rg 'libtermux-exec-wrapper|termux_exec_wrapper|termux-exec-wrappers' || echo APK_WRAPPER_ABSENT
```

期望：

```text
APK_WRAPPER_ABSENT
```

确认设备运行环境没有旧 wrapper 目录：

```bash
adb shell run-as com.termux sh -c '
cd /data/user/0/com.termux
rm -rf files/usr/libexec/termux-exec-wrappers
if [ -e files/usr/libexec/termux-exec-wrappers ]; then
  echo DEVICE_WRAPPERS_PRESENT
else
  echo DEVICE_WRAPPERS_ABSENT
fi
'
```

期望：

```text
DEVICE_WRAPPERS_ABSENT
```

## gh/git 手动验证

### 1. 验证 gh 与 git 是否可执行

在可信入口中执行：

```bash
type git
type gh
git --version
gh version
```

期望路径：

```text
git is .../files/usr/bin/git
gh is .../files/usr/bin/gh
```

如果这里失败，是 PATH/bootstrap 问题。

### 2. 验证 `gh -> git`

优先用 URL 形式绕过部分 GitHub API：

```bash
gh repo clone https://github.com/Gong-Mi/termux-app.git termux-app-url -- --depth=1
```

判读：

- 如果出现 `Cloning into ...`，说明 `gh` 已经成功启动 `git`。
- 如果出现 `fork/exec .../git: permission denied`，说明 `gh -> git` 没有走 linker，优先怀疑 Go `os/exec` 补丁。
- 如果出现 TLS/SSL/EOF，说明已到网络层。

### 3. 验证 git 自身

```bash
git clone --depth=1 https://github.com/termux/termux-packages.git termux-packages-git-direct
```

判读：

- `Cloning into ...` 后 TLS 失败：`git` 已启动，问题在网络/TLS。
- `Permission denied` 且路径是 `$PREFIX/bin/git`：exec/linker 失败。

## Go 验证方向

如果未来重新出现：

```text
failed to run git: fork/exec .../files/usr/bin/git: permission denied
```

应该验证 Go `os/exec`：

```go
package main

import (
	"fmt"
	"os/exec"
)

func main() {
	out, err := exec.Command("git", "--version").CombinedOutput()
	fmt.Printf("err=%v\n%s", err, out)
}
```

如果这个最小程序也报 `permission denied`，就不是 `gh` 补丁点，而是 Go runtime/stdlib 需要 Android/Termux exec 补丁。

需要参考 `termux-play-store/termux-packages` 的 `packages/golang` 补丁，而不是扩大 PATH wrapper。

## 已知风险

不要恢复默认 `termux-exec-wrappers` PATH 前置。实测它会破坏 `clang`：

```text
error: expected absolute path: "-cc1"
```

这类工具依赖自定位，不能用全局 wrapper 兜底。

## 后续待定

- 是否要补 Rust `readlink("/proc/self/exe")` / `realpath("/proc/self/exe")` hook。
- 是否需要把 Go `os/exec` 补丁纳入自有 package 构建。
- 网络/TLS 问题需要单独验证代理、证书和 GitHub 连通性，不应和 exec 链路混在一起。
