cmake_minimum_required(VERSION 3.18.1)
project(termux_terminal)

# --- 配置 Rust 模块 ---
set(RUST_SRC_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../rust")

if(${ANDROID_ABI} STREQUAL "arm64-v8a")
    set(RUST_TARGET "aarch64-linux-android")
elseif(${ANDROID_ABI} STREQUAL "armeabi-v7a")
    set(RUST_TARGET "armv7-linux-androideabi")
elseif(${ANDROID_ABI} STREQUAL "x86")
    set(RUST_TARGET "i686-linux-android")
elseif(${ANDROID_ABI} STREQUAL "x86_64")
    set(RUST_TARGET "x86_64-linux-android")
else()
    set(RUST_TARGET "aarch64-linux-android")
endif()

set(CARGO_TARGET_DIR "${RUST_SRC_DIR}/target")
set(RUST_LIB_NAME "libtermux_rust.so")

execute_process(COMMAND rustc -vV OUTPUT_VARIABLE RUST_HOST_INFO)
set(JNI_LIBS_DEST "${CMAKE_CURRENT_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}")

# --- 编译逻辑 ---
if(RUST_HOST_INFO MATCHES "host: ${RUST_TARGET}")
    # 本地手机原生构建
    set(CARGO_CMD "cargo")
    set(CARGO_BUILD_ARGS "build" "--release")
    set(RUST_LIB_PATH "${CARGO_TARGET_DIR}/release/${RUST_LIB_NAME}")
else()
    # 交叉编译 (GitHub CI)
    set(CARGO_CMD "cargo")
    set(CARGO_BUILD_ARGS "ndk" "-t" "${ANDROID_ABI}" "-p" "24" "build" "--release")
    set(RUST_LIB_PATH "${CARGO_TARGET_DIR}/${RUST_TARGET}/release/${RUST_LIB_NAME}")
endif()

add_custom_target(cargo_build ALL 
    COMMAND ${CARGO_CMD} ${CARGO_BUILD_ARGS}
    WORKING_DIRECTORY "${RUST_SRC_DIR}"
    COMMENT "Compiling Rust library for ${ANDROID_ABI}..."
    VERBATIM
)

# --- 产物交付 ---
add_custom_command(
    TARGET cargo_build POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E make_directory "${JNI_LIBS_DEST}"
    COMMAND ${CMAKE_COMMAND} -E copy_if_different "${RUST_LIB_PATH}" "${JNI_LIBS_DEST}/${RUST_LIB_NAME}"
    COMMENT "Delivering Rust library to ${JNI_LIBS_DEST}..."
    VERBATIM
)

# 确保库被正确注册
add_library(termux_rust SHARED IMPORTED GLOBAL)
set_target_properties(termux_rust PROPERTIES IMPORTED_LOCATION "${JNI_LIBS_DEST}/${RUST_LIB_NAME}")
add_dependencies(termux_rust cargo_build)
