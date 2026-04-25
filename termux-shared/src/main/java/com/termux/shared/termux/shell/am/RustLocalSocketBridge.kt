package com.termux.shared.termux.shell.am

import android.content.Context
import androidx.annotation.Keep
import com.termux.shared.jni.models.JniResult
import com.termux.shared.shell.am.AmSocketServer
import com.termux.shared.termux.TermuxConstants
import java.lang.ref.WeakReference

/**
 * Rust Local Socket 桥接类
 * 
 * 处理 Rust 侧 Socket 服务器的回调，并分发到 Java 侧的 Activity Manager 逻辑。
 */
@Keep
object RustLocalSocketBridge {

    private var mContext: WeakReference<Context>? = null

    @JvmStatic
    fun setContext(context: Context) {
        mContext = WeakReference(context.applicationContext)
    }

    /** 启动 Rust 侧的本地 Socket 服务器 */
    @JvmStatic
    external fun startLocalSocketServer(socketPath: String)

    /** 
     * 供 Rust 引擎回调：执行 Activity Manager 命令 
     */
    @JvmStatic
    @Keep
    fun runAmInternal(args: Array<String>): JniResult {
        val context = mContext?.get() ?: return JniResult(-1, 0, "Context not available")
        val stdout = StringBuilder()
        val stderr = StringBuilder()
        
        val error = AmSocketServer.runAmCommand(
            context, args, stdout, stderr, true
        )
        
        val result = JniResult(
            if (error == null) 0 else 1,
            0,
            error?.minimalErrorString ?: ""
        )
        
        result.stdout = stdout.toString()
        result.stderr = stderr.toString()
        
        return result
    }
}
