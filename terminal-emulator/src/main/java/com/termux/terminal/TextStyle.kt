package com.termux.terminal

/**
 * Encodes effects, foreground and background colors into a 64 bit long.
 *
 * Bit layout:
 * - 16 flags (11 currently used).
 * - 24 for foreground color (only 9 first bits if a color index).
 * - 24 for background color (only 9 first bits if a color index).
 */
object TextStyle {

    const val CHARACTER_ATTRIBUTE_BOLD = 1
    const val CHARACTER_ATTRIBUTE_ITALIC = 1 shl 1
    const val CHARACTER_ATTRIBUTE_UNDERLINE = 1 shl 2
    const val CHARACTER_ATTRIBUTE_BLINK = 1 shl 3
    const val CHARACTER_ATTRIBUTE_INVERSE = 1 shl 4
    const val CHARACTER_ATTRIBUTE_INVISIBLE = 1 shl 5
    const val CHARACTER_ATTRIBUTE_STRIKETHROUGH = 1 shl 6
    const val CHARACTER_ATTRIBUTE_PROTECTED = 1 shl 7
    const val CHARACTER_ATTRIBUTE_DIM = 1 shl 8
    const val CHARACTER_ATTRIBUTE_TRUECOLOR_FOREGROUND = 1 shl 9
    const val CHARACTER_ATTRIBUTE_TRUECOLOR_BACKGROUND = 1 shl 10

    const val COLOR_INDEX_FOREGROUND = 256
    const val COLOR_INDEX_BACKGROUND = 257
    const val COLOR_INDEX_CURSOR = 258
    const val NUM_INDEXED_COLORS = 259

    @JvmField val NORMAL: Long = encode(COLOR_INDEX_FOREGROUND, COLOR_INDEX_BACKGROUND, 0)

    @JvmStatic
    fun encode(foreColor: Int, backColor: Int, effect: Int): Long {
        var result = (effect and 0b11111111111).toLong()
        if (foreColor and 0xff000000 == 0xff000000) {
            result = result or (CHARACTER_ATTRIBUTE_TRUECOLOR_FOREGROUND.toLong() or ((foreColor.toLong() and 0x00ffffffL) shl 40))
        } else {
            result = result or ((foreColor.toLong() and 0b111111111L) shl 40)
        }
        if (backColor and 0xff000000 == 0xff000000) {
            result = result or (CHARACTER_ATTRIBUTE_TRUECOLOR_BACKGROUND.toLong() or ((backColor.toLong() and 0x00ffffffL) shl 16))
        } else {
            result = result or ((backColor.toLong() and 0b111111111L) shl 16)
        }
        return result
    }

    @JvmStatic
    fun decodeForeColor(style: Long): Int {
        return if ((style and CHARACTER_ATTRIBUTE_TRUECOLOR_FOREGROUND.toLong()) == 0L) {
            ((style ushr 40) and 0b111111111L).toInt()
        } else {
            0xff000000.toInt() or ((style ushr 40) and 0x00ffffffL).toInt()
        }
    }

    @JvmStatic
    fun decodeBackColor(style: Long): Int {
        return if ((style and CHARACTER_ATTRIBUTE_TRUECOLOR_BACKGROUND.toLong()) == 0L) {
            ((style ushr 16) and 0b111111111L).toInt()
        } else {
            0xff000000.toInt() or ((style ushr 16) and 0x00ffffffL).toInt()
        }
    }

    @JvmStatic
    fun decodeEffect(style: Long): Int = (style and 0b11111111111L).toInt()
}
