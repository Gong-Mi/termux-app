cmake_minimum_required(VERSION 3.18.1)
project(termux_app)

enable_language(C ASM)

# 目录层级：
# CMAKE_SOURCE_DIR 是 termux-app/app/src/main/cpp
# 我们需要的是 termux-app/terminal-emulator/src/main/jniLibs
set(EMULATOR_JNI_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../../../terminal-emulator/src/main/jniLibs/${ANDROID_ABI}")
set(RUST_LIB_PATH "${EMULATOR_JNI_DIR}/libtermux_rust.so")

message(STATUS "Native library path: ${RUST_LIB_PATH}")

add_library(termux-bootstrap SHARED 
    termux-bootstrap-zip.S 
    termux-bootstrap.c
)

target_include_directories(termux-bootstrap PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")
set_source_files_properties(termux-bootstrap-zip.S PROPERTIES COMPILE_FLAGS "-I${CMAKE_CURRENT_SOURCE_DIR}")

# 注意：我们这里使用静态链接占位，防止编译报错
# 但真正的打包会通过 Gradle 的 jniLibs 合并完成
target_link_libraries(termux-bootstrap log z)
