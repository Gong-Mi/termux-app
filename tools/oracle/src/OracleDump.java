package com.termux.terminal;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.FileReader;
import java.io.FileWriter;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Reference-side dumper for the differential oracle gate.
 *
 * Feeds the shared corpus (corpus/sequences/*.jsonl) into the *upstream* terminal
 * implementation and records, after every step, an exact snapshot of the observable
 * terminal state: per-cell code point + style, cursor, title, alt-screen selection and
 * transcript metrics.
 *
 * Why this exists: the Rust engine's own test suite asserts expectations written by the
 * same person who wrote the engine, so it cannot detect a *misunderstanding* of terminal
 * semantics. Comparing against an independent implementation can, and produces a concrete
 * divergence list instead of a judgement call.
 *
 * The expectation in the golden file is whatever upstream does. Upstream is pinned
 * (see tools/oracle/build.sh) so the reference cannot drift silently.
 *
 * Usage: java -cp <classes> OracleDump <corpus.jsonl> <golden.jsonl>
 */
public final class OracleDump {

    /** Upstream commit whose TerminalEmulator.java defines the expected semantics. */
    static final String UPSTREAM_PIN = "e634d8f981f48b6b89202cf0e04533f0889e03b3";

    static final int CELL_WIDTH = 10;
    static final int CELL_HEIGHT = 20;

    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            System.err.println("usage: OracleDump <corpus.jsonl> <golden.jsonl>");
            System.exit(2);
        }
        String corpusPath = args[0];
        String goldenPath = args[1];

        List<Map<String, Object>> entries = readCorpus(corpusPath);
        int snapshots = 0;
        long cells = 0;

        try (BufferedWriter out = new BufferedWriter(new FileWriter(goldenPath))) {
            for (Map<String, Object> entry : entries) {
                String id = (String) entry.get("id");
                int cols = ((Number) entry.get("cols")).intValue();
                int rows = ((Number) entry.get("rows")).intValue();
                int transcript = ((Number) entry.get("transcript")).intValue();
                @SuppressWarnings("unchecked")
                List<Object> steps = (List<Object>) entry.get("steps");

                CapturingOutput output = new CapturingOutput();
                StubClient client = new StubClient();
                TerminalEmulator emu = new TerminalEmulator(output, cols, rows,
                        CELL_WIDTH, CELL_HEIGHT, transcript, client);

                List<Map<String, Object>> marks = new ArrayList<>();
                int stepIndex = 0;
                for (Object stepObj : steps) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> step = (Map<String, Object>) stepObj;
                    String kind;
                    if (step.containsKey("send")) {
                        byte[] bytes = hexToBytes((String) step.get("send"));
                        emu.append(bytes, bytes.length);
                        kind = "send";
                    } else {
                        @SuppressWarnings("unchecked")
                        List<Object> dims = (List<Object>) step.get("resize");
                        int newCols = ((Number) dims.get(0)).intValue();
                        int newRows = ((Number) dims.get(1)).intValue();
                        emu.resize(newCols, newRows, CELL_WIDTH, CELL_HEIGHT);
                        kind = "resize";
                    }
                    Map<String, Object> mark = snapshot(emu, id, stepIndex, kind, output);
                    cells += ((Number) mark.get("cells")).longValue();
                    marks.add(mark);
                    snapshots++;
                    stepIndex++;
                }

                Map<String, Object> record = new LinkedHashMap<>();
                record.put("id", id);
                record.put("upstream_pin", UPSTREAM_PIN);
                record.put("cols", cols);
                record.put("rows", rows);
                record.put("transcript", transcript);
                record.put("snapshots", marks);
                out.write(JsonWriter.write(record));
                out.write('\n');
            }
        }

        System.out.printf("oracle: %d sequences, %d snapshots, %d compared cells -> %s%n",
                entries.size(), snapshots, cells, goldenPath);
        if (entries.isEmpty() || snapshots == 0 || cells == 0) {
            System.err.println("oracle: refusing to report success with an empty comparison");
            System.exit(3);
        }
    }

    private static Map<String, Object> snapshot(TerminalEmulator emu, String id, int stepIndex,
                                                String kind, CapturingOutput output) throws Exception {
        TerminalBuffer buf = emu.getScreen();
        // External row coordinates cover the visible screen only: `getActiveRows()` counts
        // transcript + screen rows and would address rows that do not exist externally.
        int rows = buf.mScreenRows;
        int cols = buf.mColumns;
        int firstRow = buf.externalToInternalRow(0);
        TerminalRow[] lines = buf.mLines;

        List<Object> screen = new ArrayList<>();
        long cells = 0;
        for (int row = 0; row < rows; row++) {
            int internal = buf.externalToInternalRow(row);
            TerminalRow line = (internal >= 0 && internal < lines.length) ? lines[internal] : null;
            RowProjection projection = projectRow(line, cols);
            List<Object> cpRle = new ArrayList<>();
            List<Object> stRle = new ArrayList<>();
            for (int col = 0; col < cols; col++) {
                appendRle(cpRle, projection.cells[col]);
                appendRle(stRle, buf.getStyleAt(row, col));
            }
            Map<String, Object> rowData = new LinkedHashMap<>();
            rowData.put("cp", cpRle);
            rowData.put("st", stRle);
            rowData.put("zw", projection.zeroWidth);
            screen.add(rowData);
            cells += cols;
        }

        Map<String, Object> mark = new LinkedHashMap<>();
        mark.put("step", stepIndex);
        mark.put("kind", kind);
        mark.put("cols", cols);
        mark.put("rows", rows);
        mark.put("cells", cells);
        mark.put("cursor", List.of(emu.getCursorCol(), emu.getCursorRow()));
        String title = emu.getTitle();
        mark.put("title", title == null ? "" : title);
        mark.put("alt", emu.isAlternateBufferActive());
        mark.put("transcript_rows", buf.getActiveTranscriptRows());
        mark.put("active_rows", buf.getActiveRows());
        mark.put("first_row", firstRow);
        mark.put("screen", screen);
        mark.put("transcript_hash", transcriptHash(lines, firstRow, cols));
        mark.put("client_writes", output.writes);
        return mark;
    }


    /** Cell values in the per-column projection used by the comparison. */
    static final int BLANK_CELL = ' ';
    static final int CONTINUATION_CELL = -1;

    /**
     * Project a reference row into column space.
     *
     * Upstream stores a row compactly: {@code mText} holds one slot per base character, so a
     * width-2 character covers two columns from a single slot, and zero-width characters
     * follow their base character in the same array. The Rust engine stores one slot per
     * column. Column space is the level both implementations must agree on, because it is what
     * the renderer draws and what wrapping is computed from.
     *
     * Continuation columns are *derived* from the width of the base character rather than read
     * from storage, so the comparison does not depend on which filler value an implementation
     * happens to write into the second half of a wide character.
     */
    private static RowProjection projectRow(TerminalRow line, int cols) {
        int[] cells = new int[cols];
        Arrays.fill(cells, BLANK_CELL);
        int zeroWidth = 0;
        if (line == null || line.mText == null) return new RowProjection(cells, zeroWidth);

        char[] text = line.mText;
        int used = line.getSpaceUsed();
        int charIndex = 0;
        int col = 0;
        while (charIndex < used && col < cols) {
            char c = text[charIndex++];
            int codePoint = c;
            if (Character.isHighSurrogate(c) && charIndex < used
                    && Character.isLowSurrogate(text[charIndex])) {
                codePoint = Character.toCodePoint(c, text[charIndex]);
                charIndex++;
            }
            if (codePoint == 0) {
                col++;
                continue;
            }
            int width = WcWidth.width(codePoint);
            if (width <= 0) {
                zeroWidth++;
                continue;
            }
            cells[col] = codePoint;
            for (int k = 1; k < width && col + k < cols; k++) cells[col + k] = CONTINUATION_CELL;
            col += width;
        }
        return new RowProjection(cells, zeroWidth);
    }

    /** Per-column projection of one row, plus a count of zero-width characters seen. */
    static final class RowProjection {
        final int[] cells;
        final int zeroWidth;

        RowProjection(int[] cells, int zeroWidth) {
            this.cells = cells;
            this.zeroWidth = zeroWidth;
        }
    }

    /**
     * Transcript digest. Both sides must build it identically: rows above the visible screen,
     * NUL replaced by a space, trailing spaces trimmed, joined with '\n'.
     */
    private static String transcriptHash(TerminalRow[] lines, int firstRow, int cols)
            throws Exception {
        StringBuilder sb = new StringBuilder();
        for (int internal = 0; internal < firstRow && internal < lines.length; internal++) {
            RowProjection projection = projectRow(lines[internal], cols);
            StringBuilder rowText = new StringBuilder();
            for (int col = 0; col < cols; col++) {
                int cell = projection.cells[col];
                if (cell == CONTINUATION_CELL) continue;
                rowText.appendCodePoint(cell == 0 ? ' ' : cell);
            }
            int end = rowText.length();
            while (end > 0 && rowText.charAt(end - 1) == ' ') end--;
            sb.append(rowText, 0, end);
            sb.append('\n');
        }
        MessageDigest md5 = MessageDigest.getInstance("MD5");
        byte[] digest = md5.digest(sb.toString().getBytes(StandardCharsets.UTF_8));
        StringBuilder hex = new StringBuilder();
        for (byte b : digest) hex.append(String.format("%02x", b));
        return hex.toString();
    }

    private static void appendRle(List<Object> rle, long value) {
        int size = rle.size();
        if (size >= 2 && ((Number) rle.get(size - 1)).longValue() == value) {
            rle.set(size - 2, ((Number) rle.get(size - 2)).longValue() + 1);
        } else {
            rle.add(1L);
            rle.add(value);
        }
    }

    private static byte[] hexToBytes(String hex) {
        int len = hex.length();
        if ((len & 1) != 0) throw new IllegalArgumentException("odd hex length: " + hex);
        byte[] out = new byte[len / 2];
        for (int i = 0; i < len; i += 2) {
            out[i / 2] = (byte) Integer.parseInt(hex.substring(i, i + 2), 16);
        }
        return out;
    }

    // ------------------------------------------------------------------ corpus reading

    private static List<Map<String, Object>> readCorpus(String path) throws Exception {
        List<Map<String, Object>> entries = new ArrayList<>();
        try (BufferedReader reader = new BufferedReader(new FileReader(path))) {
            String line;
            int lineNo = 0;
            while ((line = reader.readLine()) != null) {
                lineNo++;
                if (line.isBlank()) continue;
                Object parsed = new Json(line).parseValue();
                if (!(parsed instanceof Map)) {
                    throw new IllegalStateException("corpus line " + lineNo + " is not an object");
                }
                @SuppressWarnings("unchecked")
                Map<String, Object> entry = (Map<String, Object>) parsed;
                for (String required : new String[]{"id", "cols", "rows", "transcript", "steps"}) {
                    if (!entry.containsKey(required)) {
                        throw new IllegalStateException(
                                "corpus line " + lineNo + " misses field " + required);
                    }
                }
                entries.add(entry);
            }
        }
        return entries;
    }

    /** TerminalOutput stub: records everything the emulator writes back at the client. */
    static final class CapturingOutput extends TerminalOutput {
        int writes;
        StringBuilder log = new StringBuilder();

        @Override
        public void write(byte[] data, int offset, int count) {
            writes++;
            log.append(new String(data, offset, count, StandardCharsets.UTF_8));
            if (log.length() > 4096) log.setLength(4096);
        }

        @Override
        public void titleChanged(String oldTitle, String newTitle) {}

        @Override
        public void onCopyTextToClipboard(String text) {}

        @Override
        public void onPasteTextFromClipboard() {}

        @Override
        public void onBell() {}

        @Override
        public void onColorsChanged() {}
    }

    /** TerminalSessionClient stub: the emulator calls into it, so it must not be null. */
    static final class StubClient implements TerminalSessionClient {
        @Override
        public void onTextChanged(TerminalSession changedSession) {}

        @Override
        public void onTitleChanged(TerminalSession changedSession) {}

        @Override
        public void onSessionFinished(TerminalSession finishedSession) {}

        @Override
        public void onCopyTextToClipboard(TerminalSession session, String text) {}

        @Override
        public void onPasteTextFromClipboard(TerminalSession session) {}

        @Override
        public void onBell(TerminalSession session) {}

        @Override
        public void onColorsChanged(TerminalSession session) {}

        @Override
        public void onTerminalCursorStateChange(boolean state) {}

        @Override
        public void setTerminalShellPid(TerminalSession session, int pid) {}

        @Override
        public Integer getTerminalCursorStyle() {
            return TerminalEmulator.TERMINAL_CURSOR_STYLE_BLOCK;
        }

        @Override
        public void logError(String tag, String message) {}

        @Override
        public void logWarn(String tag, String message) {}

        @Override
        public void logInfo(String tag, String message) {}

        @Override
        public void logDebug(String tag, String message) {}

        @Override
        public void logVerbose(String tag, String message) {}

        @Override
        public void logStackTraceWithMessage(String tag, String message, Exception e) {}

        @Override
        public void logStackTrace(String tag, Exception e) {}
    }

    /**
     * Minimal JSON reader for the corpus format (objects, arrays, strings, numbers, null).
     * Deliberately strict: a malformed corpus line must fail loudly instead of producing a
     * golden file with silently missing steps.
     */
    static final class Json {
        private final String src;
        private int pos;

        Json(String src) {
            this.src = src;
        }

        Object parseValue() {
            skipWs();
            if (pos >= src.length()) throw err("unexpected end of input");
            char c = src.charAt(pos);
            switch (c) {
                case '{':
                    return parseObject();
                case '[':
                    return parseArray();
                case '"':
                    return parseString();
                case 't':
                    expect("true");
                    return Boolean.TRUE;
                case 'f':
                    expect("false");
                    return Boolean.FALSE;
                case 'n':
                    expect("null");
                    return null;
                default:
                    return parseNumber();
            }
        }

        private Map<String, Object> parseObject() {
            Map<String, Object> map = new LinkedHashMap<>();
            pos++;
            skipWs();
            if (peek() == '}') {
                pos++;
                return map;
            }
            while (true) {
                skipWs();
                String key = parseString();
                skipWs();
                if (peek() != ':') throw err("expected ':'");
                pos++;
                map.put(key, parseValue());
                skipWs();
                char c = peek();
                if (c == ',') {
                    pos++;
                    continue;
                }
                if (c == '}') {
                    pos++;
                    return map;
                }
                throw err("expected ',' or '}'");
            }
        }

        private List<Object> parseArray() {
            List<Object> list = new ArrayList<>();
            pos++;
            skipWs();
            if (peek() == ']') {
                pos++;
                return list;
            }
            while (true) {
                list.add(parseValue());
                skipWs();
                char c = peek();
                if (c == ',') {
                    pos++;
                    continue;
                }
                if (c == ']') {
                    pos++;
                    return list;
                }
                throw err("expected ',' or ']'");
            }
        }

        private String parseString() {
            if (peek() != '"') throw err("expected string");
            pos++;
            StringBuilder sb = new StringBuilder();
            while (true) {
                char c = src.charAt(pos++);
                if (c == '"') return sb.toString();
                if (c == '\\') {
                    char esc = src.charAt(pos++);
                    switch (esc) {
                        case '"': sb.append('"'); break;
                        case '\\': sb.append('\\'); break;
                        case '/': sb.append('/'); break;
                        case 'b': sb.append('\b'); break;
                        case 'f': sb.append('\f'); break;
                        case 'n': sb.append('\n'); break;
                        case 'r': sb.append('\r'); break;
                        case 't': sb.append('\t'); break;
                        case 'u':
                            sb.append((char) Integer.parseInt(src.substring(pos, pos + 4), 16));
                            pos += 4;
                            break;
                        default: throw err("bad escape \\" + esc);
                    }
                } else {
                    sb.append(c);
                }
            }
        }

        private Object parseNumber() {
            int start = pos;
            while (pos < src.length() && "-+.eE0123456789".indexOf(src.charAt(pos)) >= 0) pos++;
            String raw = src.substring(start, pos);
            if (raw.isEmpty()) throw err("expected number");
            if (raw.contains(".") || raw.contains("e") || raw.contains("E")) {
                return Double.parseDouble(raw);
            }
            return Long.parseLong(raw);
        }

        private void expect(String literal) {
            if (!src.startsWith(literal, pos)) throw err("expected " + literal);
            pos += literal.length();
        }

        private char peek() {
            if (pos >= src.length()) throw err("unexpected end of input");
            return src.charAt(pos);
        }

        private void skipWs() {
            while (pos < src.length() && Character.isWhitespace(src.charAt(pos))) pos++;
        }

        private IllegalStateException err(String message) {
            return new IllegalStateException(message + " at offset " + pos + " in: "
                    + src.substring(Math.max(0, pos - 20), Math.min(src.length(), pos + 20)));
        }
    }

    /** Deterministic writer: the golden file must be byte-stable across runs. */
    static final class JsonWriter {
        private JsonWriter() {}

        static String write(Object value) {
            StringBuilder sb = new StringBuilder();
            writeValue(sb, value);
            return sb.toString();
        }

        private static void writeValue(StringBuilder sb, Object value) {
            if (value == null) {
                sb.append("null");
            } else if (value instanceof String s) {
                writeString(sb, s);
            } else if (value instanceof Number || value instanceof Boolean) {
                sb.append(value);
            } else if (value instanceof Map<?, ?> map) {
                sb.append('{');
                boolean first = true;
                for (Map.Entry<?, ?> e : map.entrySet()) {
                    if (!first) sb.append(',');
                    first = false;
                    writeString(sb, String.valueOf(e.getKey()));
                    sb.append(':');
                    writeValue(sb, e.getValue());
                }
                sb.append('}');
            } else if (value instanceof List<?> list) {
                sb.append('[');
                boolean first = true;
                for (Object item : list) {
                    if (!first) sb.append(',');
                    first = false;
                    writeValue(sb, item);
                }
                sb.append(']');
            } else {
                writeString(sb, value.toString());
            }
        }

        private static void writeString(StringBuilder sb, String s) {
            sb.append('"');
            for (int i = 0; i < s.length(); i++) {
                char c = s.charAt(i);
                switch (c) {
                    case '"': sb.append("\\\""); break;
                    case '\\': sb.append("\\\\"); break;
                    case '\n': sb.append("\\n"); break;
                    case '\r': sb.append("\\r"); break;
                    case '\t': sb.append("\\t"); break;
                    default:
                        if (c < 0x20) {
                            sb.append(String.format("\\u%04x", (int) c));
                        } else {
                            sb.append(c);
                        }
                }
            }
            sb.append('"');
        }
    }
}