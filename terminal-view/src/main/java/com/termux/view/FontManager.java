package com.termux.view;

import android.content.Context;
import android.graphics.Typeface;
import android.util.Log;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;

public class FontManager {

    private static final String FONT_NAME = "dejavu_sans_mono.ttf";
    private static final String FONT_ASSET_PATH = "fonts/" + FONT_NAME;
    private static Typeface customTypeface;

    public static Typeface getTypeface(Context context) {
        if (customTypeface == null) {
            File fontFile = new File(context.getCacheDir(), FONT_NAME);
            if (!fontFile.exists()) {
                copyFontFromAssets(context, fontFile);
            }

            if (fontFile.exists()) {
                try {
                    customTypeface = Typeface.createFromFile(fontFile);
                } catch (Exception e) {
                    Log.e("FontManager", "Failed to create typeface from file", e);
                }
            }
        }

        if (customTypeface != null) {
            return customTypeface;
        } else {
            return Typeface.MONOSPACE;
        }
    }

    private static void copyFontFromAssets(Context context, File destination) {
        try (InputStream inputStream = context.getAssets().open(FONT_ASSET_PATH);
             FileOutputStream outputStream = new FileOutputStream(destination)) {

            byte[] buffer = new byte[1024];
            int length;
            while ((length = inputStream.read(buffer)) > 0) {
                outputStream.write(buffer, 0, length);
            }
        } catch (Exception e) {
            Log.e("FontManager", "Failed to copy font from assets", e);
        }
    }
}
