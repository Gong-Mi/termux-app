package android.graphics;

/**
 * Minimal stand-in for the Android framework class, used only to compile the reference
 * terminal implementation (upstream Termux `TerminalColors.java`, which needs
 * red()/green()/blue() for its perceived-brightness calculation) on a plain JVM.
 *
 * Semantics match the documented Android behaviour for the members used by the reference
 * code, so the oracle's colour decisions stay faithful to upstream.
 */
public final class Color {

    private Color() {}

    public static int alpha(int color) { return color >>> 24; }

    public static int red(int color) { return (color >> 16) & 0xFF; }

    public static int green(int color) { return (color >> 8) & 0xFF; }

    public static int blue(int color) { return color & 0xFF; }

    public static int rgb(int red, int green, int blue) {
        return 0xFF000000 | (red << 16) | (green << 8) | blue;
    }

    public static int argb(int alpha, int red, int green, int blue) {
        return (alpha << 24) | (red << 16) | (green << 8) | blue;
    }

    /** Supports #RGB, #RRGGBB, #AARRGGBB as documented for the framework. */
    public static int parseColor(String colorString) {
        if (colorString.charAt(0) != '#') {
            throw new IllegalArgumentException("Unknown color: " + colorString);
        }
        long color;
        switch (colorString.length()) {
            case 4: {
                long c = Long.parseLong(colorString.substring(1), 16);
                long r = (c & 0xF00) >> 8, g = (c & 0x0F0) >> 4, b = c & 0x00F;
                color = 0xFF000000L | (r << 20) | (r << 16) | (g << 12) | (g << 8) | (b << 4) | b;
                break;
            }
            case 7: {
                color = Long.parseLong(colorString.substring(1), 16) | 0xFF000000L;
                break;
            }
            case 9: {
                color = Long.parseLong(colorString.substring(1), 16);
                break;
            }
            default:
                throw new IllegalArgumentException("Unknown color: " + colorString);
        }
        return (int) color;
    }
}