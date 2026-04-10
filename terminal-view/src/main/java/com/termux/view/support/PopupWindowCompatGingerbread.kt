/*
 * Copyright (C) 2015 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License
 */
package com.termux.view.support

import android.widget.PopupWindow
import java.lang.reflect.Method

/**
 * Implementation of PopupWindow compatibility that can call Gingerbread APIs.
 */
object PopupWindowCompatGingerbread {

    private var sSetWindowLayoutTypeMethod: Method? = null
    private var sSetWindowLayoutTypeMethodAttempted = false
    private var sGetWindowLayoutTypeMethod: Method? = null
    private var sGetWindowLayoutTypeMethodAttempted = false

    @JvmStatic
    fun setWindowLayoutType(popupWindow: PopupWindow, layoutType: Int) {
        if (!sSetWindowLayoutTypeMethodAttempted) {
            sSetWindowLayoutTypeMethod = runCatching {
                PopupWindow::class.java.getDeclaredMethod("setWindowLayoutType", Int::class.javaPrimitiveType)
                    .apply { isAccessible = true }
            }.getOrNull()
            sSetWindowLayoutTypeMethodAttempted = true
        }
        sSetWindowLayoutTypeMethod?.runCatching { invoke(popupWindow, layoutType) }
    }

    @JvmStatic
    fun getWindowLayoutType(popupWindow: PopupWindow): Int {
        if (!sGetWindowLayoutTypeMethodAttempted) {
            sGetWindowLayoutTypeMethod = runCatching {
                PopupWindow::class.java.getDeclaredMethod("getWindowLayoutType")
                    .apply { isAccessible = true }
            }.getOrNull()
            sGetWindowLayoutTypeMethodAttempted = true
        }
        return sGetWindowLayoutTypeMethod?.runCatching { invoke(popupWindow) as Int }?.getOrNull() ?: 0
    }
}
