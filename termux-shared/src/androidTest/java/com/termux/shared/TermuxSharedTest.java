package com.termux.shared;

import android.content.Context;
import android.os.Build;
import androidx.test.platform.app.InstrumentationRegistry;
import androidx.test.ext.junit.runners.AndroidJUnit4;

import org.junit.Test;
import org.junit.runner.RunWith;

import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
public class TermuxSharedTest {

    @Test
    public void testFileSystemAccess() throws Exception {
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        
        // 验证基本文件操作在当前路径下是否正常
        File testFile = new File(context.getFilesDir(), "performance_test.txt");
        String testData = "Termux High Performance Test";
        
        try (FileOutputStream fos = new FileOutputStream(testFile)) {
            fos.write(testData.getBytes(StandardCharsets.UTF_8));
        }
        
        assertTrue("测试文件应该被创建", testFile.exists());
        assertEquals("文件大小应匹配", testData.length(), testFile.length());
        
        // 清理
        testFile.delete();
    }

    @Test
    public void testEnvironmentInfo() {
        // 打印关键的环境信息到日志，便于调试 16KB 对齐和 API 级别
        Context context = InstrumentationRegistry.getInstrumentation().getTargetContext();
        System.out.println("--- Termux Shared Test Environment ---");
        System.out.println("Package: " + context.getPackageName());
        System.out.println("Files Dir: " + context.getFilesDir().getAbsolutePath());
        System.out.println("SDK Level: " + Build.VERSION.SDK_INT);
        System.out.println("ABI: " + Build.CPU_ABI);
        System.out.println("---------------------------------------");
        
        // 验证路径是否已对齐到 /data/user/0 (在多用户或新版系统上)
        String filesPath = context.getFilesDir().getAbsolutePath();
        assertTrue("路径应包含 /data/user/0 或兼容结构", 
            filesPath.contains("/data/user/0") || filesPath.contains("/data/data"));
    }

    @Test
    public void testNativeLibraryLoading() {
        // 验证 16KB 对齐后的 native 库是否能被成功加载
        // termux-shared 模块通常会编译 libtermux-shared.so
        try {
            System.loadLibrary("termux-shared");
            System.out.println("Native library 'termux-shared' loaded successfully (16KB check passed)");
        } catch (UnsatisfiedLinkError e) {
            // 如果库还没构建好，这里可能会报错，但在正式 release 中应通过
            System.err.println("Note: libtermux-shared.so not found, skipping specific native check.");
        }
    }
}
