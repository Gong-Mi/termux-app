package com.termux.terminal;

/**
 * Implementation of wcwidth(3) for Unicode 15.
 *
 * @deprecated All logic has been migrated to Rust.
 * This class now delegates to Rust implementation for backward compatibility.
 * 
 * Implementation from https://github.com/jquast/wcwidth but we return 0 for unprintable characters.
 *
 * IMPORTANT:
 * Must be kept in sync with the following:
 * https://github.com/termux/wcwidth
 * https://github.com/termux/libandroid-support
 * https://github.com/termux/termux-packages/tree/master/packages/libandroid-support
 */
@Deprecated
public final class WcWidth {

    private static native int widthRust(int ucs);

    /** Return the terminal display width of a code point: 0, 1 or 2. */
    public static int width(int ucs) {
        // Always use Rust implementation
        if (JNI.sNativeLibrariesLoaded) {
            return widthRust(ucs);
        }
        // Fallback should not happen in normal operation
        throw new RuntimeException("Rust native library not loaded");
    }

    /** The width at an index position in a java char array. */
    public static int width(char[] chars, int index) {
        char c = chars[index];
        return Character.isHighSurrogate(c) ? width(Character.toCodePoint(c, chars[index + 1])) : width(c);
    }

    /**
     * The zero width characters count like combining characters in the `chars` array from start
     * index to end index (exclusive).
     */
    public static int zeroWidthCharsCount(char[] chars, int start, int end) {
        if (start < 0 || start >= chars.length)
            return 0;

        int count = 0;
        for (int i = start; i < end && i < chars.length;) {
            if (Character.isHighSurrogate(chars[i])) {
                if (width(Character.toCodePoint(chars[i], chars[i + 1])) <= 0) {
                    count++;
                }
                i += 2;
            } else {
                if (width(chars[i]) <= 0) {
                    count++;
                }
                i++;
            }
        }
        return count;
    }

}
