LOCAL_PATH:= $(call my-dir)

# --- 引入外部编译好的 Rust 库 (libtermux_rust.so) ---
include $(CLEAR_VARS)
LOCAL_MODULE := termux_rust
# 路径指向 CMake 生成的 jniLibs 目录，由 Gradle ABI 自动探测
LOCAL_SRC_FILES := ../../../terminal-emulator/src/main/jniLibs/$(TARGET_ARCH_ABI)/libtermux_rust.so
include $(BUILD_SHARED_LIBRARY)

# --- 编译原本的 Bootstrap 库 ---
include $(CLEAR_VARS)
LOCAL_MODULE := libtermux-bootstrap
LOCAL_SRC_FILES := termux-bootstrap-zip.S termux-bootstrap.c
include $(BUILD_SHARED_LIBRARY)
