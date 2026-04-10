package com.termux.terminal

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
object WcWidth {

    @JvmStatic external fun widthRust(ucs: Int): Int

    /** Return the terminal display width of a code point: 0, 1 or 2. */
    @JvmStatic
    fun width(ucs: Int): Int {
        if (JNI.sNativeLibrariesLoaded) {
            return widthRust(ucs)
        }
        // Fallback for unit tests: use simple heuristic
        if (ucs < 32 || (ucs >= 0x7F && ucs < 0xA0)) return 0
        if (ucs >= 0x1100 && isWideCharacter(ucs)) return 2
        return 1
    }

    /** Simple check for wide characters (CJK, etc.) - fallback for tests only. */
    private fun isWideCharacter(ucs: Int): Boolean =
        ucs in 0x4E00..0x9FFF ||
        ucs in 0xF900..0xFAFF ||
        ucs in 0x3400..0x4DBF ||
        ucs in 0xFF01..0xFF60 ||
        ucs in 0x3000..0x303F

    /** The width at an index position in a java char array. */
    @JvmStatic
    fun width(chars: CharArray, index: Int): Int {
        val c = chars[index]
        return if (Character.isHighSurrogate(c))
            width(Character.toCodePoint(c, chars[index + 1]))
        else
            width(c.code)
    }

    /**
     * The zero width characters count like combining characters in the `chars` array from start
     * index to end index (exclusive).
     */
    @JvmStatic
    fun zeroWidthCharsCount(chars: CharArray, start: Int, end: Int): Int {
        if (start < 0 || start >= chars.size) return 0
        var count = 0
        var i = start
        while (i < end && i < chars.size) {
            if (Character.isHighSurrogate(chars[i])) {
                if (width(Character.toCodePoint(chars[i], chars[i + 1])) <= 0) count++
                i += 2
            } else {
                if (width(chars[i].code) <= 0) count++
                i++
            }
        }
        return count
    }
}
