package android.util;

import java.nio.charset.StandardCharsets;

/**
 * Minimal stand-in for the Android framework class, used only to compile the reference
 * terminal implementation on a plain JVM.
 *
 * The reference emulator decodes OSC 52 clipboard payloads with `Base64.decode(text, 0)`,
 * so this stub has to be semantically correct rather than a no-op: OSC 52 handling is one
 * of the behaviours the differential gate compares. Flag handling follows the documented
 * Android contract for the flags the reference code uses.
 */
public final class Base64 {

    private Base64() {}

    public static final int DEFAULT = 0;
    public static final int NO_PADDING = 1;
    public static final int NO_WRAP = 2;
    public static final int CRLF = 4;
    public static final int URL_SAFE = 8;
    public static final int NO_CLOSE = 16;

    public static byte[] decode(String str, int flags) {
        String cleaned = str.replaceAll("\\s", "");
        if ((flags & NO_PADDING) != 0) {
            while (cleaned.endsWith("=")) {
                cleaned = cleaned.substring(0, cleaned.length() - 1);
            }
        }
        int padded = cleaned.length() % 4;
        if (padded != 0) {
            StringBuilder sb = new StringBuilder(cleaned);
            for (int i = padded; i < 4; i++) sb.append('=');
            cleaned = sb.toString();
        }
        java.util.Base64.Decoder decoder = ((flags & URL_SAFE) != 0)
                ? java.util.Base64.getUrlDecoder()
                : java.util.Base64.getMimeDecoder();
        return decoder.decode(cleaned.getBytes(StandardCharsets.US_ASCII));
    }

    public static byte[] decode(byte[] input, int flags) {
        return decode(new String(input, StandardCharsets.US_ASCII), flags);
    }

    public static String encodeToString(byte[] input, int flags) {
        java.util.Base64.Encoder encoder = ((flags & URL_SAFE) != 0)
                ? java.util.Base64.getUrlEncoder()
                : java.util.Base64.getEncoder();
        if ((flags & NO_PADDING) != 0) {
            encoder = encoder.withoutPadding();
        }
        return encoder.encodeToString(input);
    }
}