LOCAL_PATH:= $(call my-dir)
include $(CLEAR_VARS)
LOCAL_MODULE:= libtermux
LOCAL_SRC_FILES:= termux.c
include $(BUILD_SHARED_LIBRARY)

include $(CLEAR_VARS)
LOCAL_MODULE:= libtermux-vulkan
LOCAL_SRC_FILES:= vulkan_renderer.cpp
LOCAL_LDLIBS += -llog -landroid -lEGL -lGLESv2 -lvulkan
LOCAL_CPPFLAGS += -Wno-missing-braces -Wno-unused-parameter

include $(BUILD_SHARED_LIBRARY)
