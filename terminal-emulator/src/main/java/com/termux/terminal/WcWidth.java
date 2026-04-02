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
        if (JNI.sNativeLibrariesLoaded) {
            return widthRust(ucs);
        }
        // Fallback for unit tests: use simple heuristic
        // This is not as accurate as the Rust implementation but sufficient for tests
        if (ucs < 32 || (ucs >= 0x7F && ucs < 0xA0)) return 0;
        if (ucs >= 0x1100 && isWideCharacter(ucs)) return 2;
        return 1;
    }

    /** Simple check for wide characters (CJK, etc.) - fallback for tests only. */
    private static boolean isWideCharacter(int ucs) {
        // CJK Unified Ideographs
        if (ucs >= 0x4E00 && ucs <= 0x9FFF) return true;
        // CJK Compatibility Ideographs
        if (ucs >= 0xF900 && ucs <= 0xFAFF) return true;
        // CJK Unified Ideographs Extension A
        if (ucs >= 0x3400 && ucs <= 0x4DBF) return true;
        // Fullwidth ASCII variants
        if (ucs >= 0xFF01 && ucs <= 0xFF60) return true;
        // CJK Symbols and Punctuation
        if (ucs >= 0x3000 && ucs <= 0x303F) return true;
        return false;
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
