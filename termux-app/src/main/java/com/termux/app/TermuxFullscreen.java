package com.termux.app;

import android.view.WindowInsets;
import android.view.WindowManager;

import com.termux.R;

/**
 * See <a href="https://developer.android.com/develop/ui/views/layout/insets/rounded-corners">Insets: Apply rounded corners</a>
 * and <a href="https://developer.android.com/develop/ui/views/layout/sw-keyboard">Control and animate the software keyboard</a>.
 */
public class TermuxFullscreen {

    private static final boolean CORNERS_API = (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S);

    private static native int[] nativeCalculatePadding(
        boolean fullscreen,
        int statusBarTop, int imeBottom,
        int cornerTopLeft, int cornerTopRight,
        int cornerBottomRight, int cornerBottomLeft,
        int windowTopMargin, int windowBottomMargin
    );

    static {
        try {
            System.loadLibrary("termux_rust");
        } catch (UnsatisfiedLinkError e) {
            android.util.Log.w("TermuxFullscreen", "libtermux_rust not loaded yet, deferring to runtime");
        }
    }

    public static void updatePadding(TermuxActivity activity, WindowInsets insets) {
        var rootView = activity.findViewById(R.id.activity_termux_root_relative_layout);
        boolean fullscreen = activity.mPreferences.isFullscreen();

        int statusBarTop = insets.getInsets(WindowInsets.Type.statusBars()).top;
        int imeBottom = insets.getInsets(WindowInsets.Type.ime()).bottom;

        int cornerTL = cornerRadius(insets, 0);
        int cornerTR = cornerRadius(insets, 1);
        int cornerBR = cornerRadius(insets, 2);
        int cornerBL = cornerRadius(insets, 3);

        int topMargin = 0, bottomMargin = 0;
        if (fullscreen) {
            var windowManager = activity.getSystemService(WindowManager.class);
            var windowBounds = windowManager.getCurrentWindowMetrics().getBounds();
            int[] location = {0, 0};
            rootView.getLocationInWindow(location);
            topMargin = location[1] - windowBounds.top;
            bottomMargin = Math.max(0, windowBounds.bottom - rootView.getBottom());
        }

        int[] padding;
        try {
            padding = nativeCalculatePadding(
                fullscreen,
                statusBarTop, imeBottom,
                cornerTL, cornerTR, cornerBR, cornerBL,
                topMargin, bottomMargin
            );
        } catch (UnsatisfiedLinkError e) {
            // Fallback: native lib not yet loaded, compute in Java
            padding = fallbackCalculatePadding(
                fullscreen, statusBarTop, imeBottom,
                cornerTL, cornerTR, cornerBR, cornerBL,
                topMargin, bottomMargin
            );
        }
        rootView.setPadding(padding[0], padding[1], padding[2], padding[3]);
    }

    private static int[] fallbackCalculatePadding(
        boolean fullscreen,
        int statusBarTop, int imeBottom,
        int cornerTL, int cornerTR, int cornerBR, int cornerBL,
        int topMargin, int bottomMargin
    ) {
        if (fullscreen) {
            int topPadding = Math.max(statusBarTop,
                Math.max(Math.max(cornerTL, cornerTR) - topMargin, 0));
            int bottomPadding = Math.max(imeBottom,
                Math.max(Math.max(cornerBL, cornerBR) - bottomMargin, 0));
            return new int[]{0, topPadding, 0, bottomPadding};
        } else {
            return new int[]{0, 0, 0, imeBottom};
        }
    }

    private static int cornerRadius(WindowInsets insets, int position) {
        if (CORNERS_API) {
            var corner = insets.getRoundedCorner(position);
            return corner == null ? 0 : corner.getRadius();
        } else {
            return 0;
        }
    }

}
