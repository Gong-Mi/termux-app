#!/usr/bin/env python3
"""Generate the differential-oracle corpus for the Termux Rust terminal engine.

The corpus is a machine-readable list of byte sequences ("steps") that are fed to two
independent implementations of the same terminal semantics:

  * the reference implementation: upstream Termux `TerminalEmulator.java` at the pinned
    baseline commit (v0.119.0-beta.3 = e634d8f981f48b6b89202cf0e04533f0889e03b3), and
  * the Rust engine under test (`termux-rust-new`).

Neither side owns the expectations: the expectation is whatever the reference produces.
This is the point of the harness -- self-authored expectations cannot find an author's
own misunderstanding.

Output: corpus/sequences/seed.jsonl, one JSON object per line:

  {"id": "...", "desc": "...", "cols": 80, "rows": 24, "transcript": 100,
   "steps": [{"send": "<hex>"}, {"resize": [c, r]}, ...]}

`send` is lowercase hex so that both the JVM and the Rust side parse it identically
without any escaping rules. Steps are applied in order and the state is snapshotted
after every step, so a divergence can be attributed to the exact step that caused it.

Run: python3 scripts/gen_corpus_seed.py
"""

from __future__ import annotations

import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "corpus" / "sequences" / "seed.jsonl"

ESC = b"\x1b"
CSI = ESC + b"["
OSC = ESC + b"]"
DCS = ESC + b"P"
ST = ESC + b"\\"
BEL = b"\x07"

DEFAULT_COLS = 80
DEFAULT_ROWS = 24
DEFAULT_TRANSCRIPT = 100


def s(b: bytes) -> dict:
    return {"send": b}


def r(cols: int, rows: int) -> dict:
    return {"resize": [cols, rows]}


def entry(id_, desc, steps, cols=DEFAULT_COLS, rows=DEFAULT_ROWS, transcript=DEFAULT_TRANSCRIPT):
    return {
        "id": id_,
        "desc": desc,
        "cols": cols,
        "rows": rows,
        "transcript": transcript,
        "steps": steps,
    }


def sgr(*params: str) -> bytes:
    return CSI + b";".join(p.encode() for p in params) + b"m"


def csi(seq: str) -> bytes:
    return CSI + seq.encode()


def fill_lines(n: int) -> bytes:
    return b"".join(b"line%02d\r\n" % i for i in range(n))


def entries() -> list[dict]:
    e: list[dict] = []

    # --- 1. plain text, control characters, wrapping -------------------------------
    e.append(entry("text-basic", "plain ASCII text", [s(b"Hello, World!")]))
    e.append(entry("text-crlf", "CR LF line break", [s(b"line1\r\nline2\r\nline3")]))
    e.append(entry("text-lf-only", "LF only keeps column (no CR)", [s(b"abc\ndef")]))
    e.append(entry("text-cr-overwrite", "carriage return overwrite (progress bar)",
                   [s(b"progress: 10%\rprogress: 99%")]))
    e.append(entry("text-backspace", "backspace moves cursor back",
                   [s(b"abcdef\b\bXY")]))
    e.append(entry("text-tab", "tab advances to 8-column stops",
                   [s(b"a\tb\tc\t1\t2345678\tZ")]))
    e.append(entry("text-wrap-auto", "autowrap over 80 columns",
                   [s(b"x" * 85)]))
    e.append(entry("text-wrap-disabled", "DECRST 7 disables autowrap",
                   [s(csi("?7l")), s(b"x" * 85)]))
    e.append(entry("text-long-lines", "several wrapped lines",
                   [s((b"0123456789" * 9 + b"\r\n") * 3)]))

    # --- 2. cursor addressing and motion ------------------------------------------
    e.append(entry("cursor-cup", "CUP direct addressing",
                   [s(csi("5;10H") + b"X" + csi("H") + b"HOME" + csi("2;3f") + b"F")]))
    e.append(entry("cursor-motion", "CUU/CUD/CUF/CUB/CNL/CPL",
                   [s(csi("10;10H") + b"A" + csi("3A") + b"B" + csi("4B") + b"C"
                      + csi("5C") + b"D" + csi("2D") + b"E" + csi("2E") + b"F"
                      + csi("3F") + b"G")]))
    e.append(entry("cursor-cha-vpa", "CHA / VPA absolute column and row",
                   [s(csi("20G") + b"col" + csi("5d") + b"row")]))
    e.append(entry("cursor-hvp", "HVP (CUP alias)", [s(csi("4;7f") + b"Z")]))
    e.append(entry("cursor-save-restore-esc", "ESC 7 / ESC 8 save-restore cursor",
                   [s(b"one" + ESC + b"7" + b"two" + ESC + b"8" + b"X")]))
    e.append(entry("cursor-save-restore-csi", "CSI s / CSI u save-restore cursor",
                   [s(b"one" + csi("s") + b"two" + csi("u") + b"X")]))
    e.append(entry("cursor-decset-1048", "DECSET 1048 save cursor mode",
                   [s(b"one" + csi("?1048h") + b"two" + csi("?1048l") + b"X")]))
    e.append(entry("cursor-visibility", "DECTCEM cursor visibility toggles",
                   [s(csi("?25l") + b"hidden" + csi("?25h") + b"shown")]))

    # --- 3. erasing / editing ------------------------------------------------------
    for mode in (0, 1, 2, 3):
        e.append(entry(f"erase-display-{mode}", f"ED {mode}",
                       [s(fill_lines(8)), s(csi("4;20H")), s(csi(f"{mode}J"))]))
    for mode in (0, 1, 2):
        e.append(entry(f"erase-line-{mode}", f"EL {mode}",
                       [s(fill_lines(6)), s(csi("3;15H")), s(csi(f"{mode}K"))]))
    e.append(entry("erase-chars", "ECH erases cells without shifting",
                   [s(b"abcdefghij" + csi("1;3H") + csi("4X"))]))
    e.append(entry("insert-chars", "ICH inserts blanks", [s(b"abcdefghij" + csi("3@"))]))
    e.append(entry("delete-chars", "DCH deletes cells", [s(b"abcdefghij" + csi("3P"))]))
    e.append(entry("insert-lines", "IL inserts blank lines",
                   [s(fill_lines(6)), s(csi("2;1H")), s(csi("2L"))]))
    e.append(entry("delete-lines", "DL removes lines",
                   [s(fill_lines(6)), s(csi("2;1H")), s(csi("2M"))]))
    e.append(entry("scroll-up-su", "SU scrolls region up", [s(fill_lines(6)), s(csi("3S"))]))
    e.append(entry("scroll-down-sd", "SD scrolls region down", [s(fill_lines(6)), s(csi("3T"))]))
    e.append(entry("insert-mode-irm", "IRM inserts typed text",
                   [s(b"abcdefgh"), s(csi("4h")), s(b"123"), s(csi("4l"))]))

    # --- 4. margins and scrolling regions -----------------------------------------
    e.append(entry("decstbm-scroll", "DECSTBM with scrolling text",
                   [s(csi("5;10r")), s(fill_lines(12)), s(csi("r")), s(b"after-reset")]))
    e.append(entry("decstbm-il-dl", "IL/DL inside a DECSTBM region",
                   [s(csi("4;8r")), s(fill_lines(10)), s(csi("5;1H")), s(csi("2L")),
                    s(csi("2M"))]))
    e.append(entry("decslrm", "DECSLRM left/right margins (mode 69)",
                   [s(csi("?69h")), s(csi("1;20s")), s(fill_lines(4)), s(csi("?69l"))]))
    e.append(entry("origin-mode", "DECOM origin mode with DECSTBM",
                   [s(csi("3;9r")), s(csi("?6h")), s(csi("H") + b"origin-home"), s(csi("?6l"))]))
    e.append(entry("index-next-line", "IND / NEL / RI index controls",
                   [s(b"first" + ESC + b"D" + b"index" + ESC + b"E" + b"nel"
                      + ESC + b"M" + b"rev")]))
    e.append(entry("reverse-index-margin", "RI at top margin scrolls down",
                   [s(csi("2;6r")), s(fill_lines(6)), s(csi("2;1H")), s(ESC + b"M")]))

    # --- 5. character attributes (SGR) --------------------------------------------
    e.append(entry("sgr-attributes", "each supported attribute then reset",
                   [s(b"".join(sgr(p) + n.encode() + sgr("0") for p, n in
                               [("1", "bold"), ("2", "dim"), ("3", "ital"), ("4", "under"),
                                ("5", "blink"), ("7", "rev"), ("8", "invis"), ("9", "strike")]))]))
    e.append(entry("sgr-attr-off", "individual attribute resets",
                   [s(sgr("1", "3", "4", "9") + b"all-on"), s(sgr("22", "23", "24", "29") + b"off")]))
    e.append(entry("sgr-16color", "basic and bright 16 colors",
                   [s(sgr("31") + b"red" + sgr("32") + b"green" + sgr("44") + b"bgblue"
                      + sgr("0") + sgr("91") + b"brightred" + sgr("107") + b"bgwhite" + sgr("0"))]))
    e.append(entry("sgr-256", "256-color indexed fg/bg",
                   [s(sgr("38", "5", "196") + b"idx196" + sgr("48", "5", "22") + b"bg22"
                      + sgr("38", "5", "250") + b"idx250" + sgr("0"))]))
    e.append(entry("sgr-truecolor", "24-bit truecolor semicolon form",
                   [s(sgr("38", "2", "255", "128", "0") + b"orange"
                      + sgr("48", "2", "10", "20", "30") + b"darkbg" + sgr("0"))]))
    e.append(entry("sgr-truecolor-colon", "24-bit truecolor colon form",
                   [s(csi("38:2::12:34:56m") + b"colon" + csi("48:2::1:2:3m") + b"bg"
                      + sgr("0"))]))
    e.append(entry("sgr-default-colors", "SGR 39/49 default fg/bg",
                   [s(sgr("31", "44") + b"colored" + sgr("39", "49") + b"default" + sgr("0"))]))
    e.append(entry("sgr-reverse-screen", "DECSCNM reverse video mode",
                   [s(b"normal" + csi("?5h") + b"reversed" + csi("?5l"))]))
    e.append(entry("sgr-reset-and-persist", "style persists across cells and is reset",
                   [s(sgr("1", "4") + b"styled text" + sgr("0") + b" plain")]))

    # --- 6. screen buffers (alt screen) -------------------------------------------
    e.append(entry("altscreen-1049", "DECSET 1049 alt screen with cursor save",
                   [s(b"main content\r\n"), s(csi("?1049h")), s(b"alt content"),
                    s(csi("?1049l"))]))
    e.append(entry("altscreen-47", "DECSET 47 alt screen",
                   [s(b"main\r\n"), s(csi("?47h")), s(b"alt"), s(csi("?47l"))]))
    e.append(entry("altscreen-1047", "DECSET 1047 alt screen clears on entry",
                   [s(b"main\r\n"), s(csi("?1047h")), s(b"alt"), s(csi("?1047l"))]))
    e.append(entry("altscreen-editor", "vim-like alt screen edit",
                   [s(b"shell prompt\r\n"), s(csi("?1049h")), s(csi("H")),
                    s(sgr("7") + b"~" + sgr("0") + b"\r\n"), s(b":wq\r\n"), s(csi("?1049l"))]))

    # --- 7. OSC --------------------------------------------------------------------
    e.append(entry("osc-title-bel", "OSC 0 title terminated by BEL",
                   [s(OSC + b"0;my-title" + BEL)]))
    e.append(entry("osc-title-st", "OSC 2 title terminated by ST, OSC 1 icon",
                   [s(OSC + b"1;icon" + ST), s(OSC + b"2;second-title" + ST)]))
    e.append(entry("osc-title-stack", "OSC 22 push / OSC 23 pop title",
                   [s(OSC + b"0;one" + BEL), s(OSC + b"22;two" + BEL), s(OSC + b"23;x" + BEL)]))
    e.append(entry("osc-colors-fg-bg", "OSC 10/11 set fg/bg",
                   [s(OSC + b"10;rgb:ff/00/00" + BEL), s(OSC + b"11;rgb:00/00/ff" + BEL),
                    s(b"colored by OSC")]))
    e.append(entry("osc-palette", "OSC 4 palette redefinition",
                   [s(OSC + b"4;1;rgb:ff/ff/00" + BEL), s(sgr("31") + b"palette1" + sgr("0"))]))
    e.append(entry("osc-52-clipboard-set", "OSC 52 clipboard set (Base64 payload)",
                   [s(OSC + b"52;c;aGVsbG8gd29ybGQ=" + ST)]))
    e.append(entry("osc-52-clipboard-query", "OSC 52 clipboard query", [s(OSC + b"52;c;?" + BEL)]))
    e.append(entry("osc-8-hyperlink", "OSC 8 hyperlink wrapper",
                   [s(OSC + b"8;;https://example.com" + ST + b"link text" + OSC + b"8;;" + ST)]))
    e.append(entry("osc-unknown", "unknown OSC is ignored",
                   [s(OSC + b"1234;payload" + BEL), s(b"still fine")]))

    # --- 8. DCS / APC / device queries --------------------------------------------
    e.append(entry("dcs-unknown", "unknown DCS is consumed and ignored",
                   [s(DCS + b"0+q" + ST), s(b"after-dcs")]))
    e.append(entry("dsr-cursor-report", "DSR 6n cursor position report",
                   [s(csi("5;7H")), s(csi("6n"))]))
    e.append(entry("da-primary", "DA1 device attributes query", [s(csi("c"))]))
    e.append(entry("da-secondary", "DA2 device attributes query", [s(csi(">c"))]))
    e.append(entry("xtversion", "XTVERSION query", [s(csi(">0q"))]))
    e.append(entry("decaln", "DECALN screen alignment test", [s(ESC + b"#8"), s(b"after")]))

    # --- 9. charsets / line drawing ------------------------------------------------
    e.append(entry("acs-line-drawing", "DEC special graphics line drawing",
                   [s(ESC + b"(0" + b"lqqk\r\nx   x\r\nmqqj" + ESC + b"(B")]))
    e.append(entry("charset-uk", "UK charset designation",
                   [s(ESC + b"(A" + b"#[]{}" + ESC + b"(B")]))
    e.append(entry("charset-shift", "SO/SI shift between G0 and G1",
                   [s(ESC + b")0" + b"a\x0elqqk\x0fb" + ESC + b"(B")]))

    # --- 10. tabs, modes, misc -----------------------------------------------------
    e.append(entry("tab-stops-hts", "HTS set tab stop, TBC clear",
                   [s(b"a\tb\tc"), s(ESC + b"H"), s(b"\tD"), s(csi("3g")), s(b"\tE")]))
    e.append(entry("mouse-modes", "mouse tracking mode toggles",
                   [s(csi("?1000h") + csi("?1002h") + csi("?1006h") + b"mousemodes"
                      + csi("?1000l") + csi("?1002l") + csi("?1006l"))]))
    e.append(entry("bracketed-paste-mode", "bracketed paste mode toggle",
                   [s(csi("?2004h") + b"paste" + csi("?2004l"))]))
    e.append(entry("bell", "BEL is a client event, screen unchanged",
                   [s(b"before" + BEL + b"after")]))
    e.append(entry("scrollback-fill", "transcript overflow with small screen",
                   [s(fill_lines(60))], rows=6, transcript=20))
    e.append(entry("scrollback-clear", "ED 2 then transcript state",
                   [s(fill_lines(40)), s(csi("2J"))], rows=8, transcript=20))

    # --- 11. wide / combining characters ------------------------------------------
    e.append(entry("wide-cjk", "CJK wide glyphs and wrap",
                   [s("中文日本語テスト".encode()), s(b"\r\n"), s("漢字".encode())]))
    e.append(entry("wide-cjk-narrow", "wide glyphs in a 10-column screen",
                   [s("中文中文中".encode())], cols=10, rows=6))
    e.append(entry("wide-mixed-ascii", "ASCII inside CJK line",
                   [s("start 中 end".encode())]))
    e.append(entry("combining-marks", "combining acute/cedilla overlay",
                   [s("e\u0301a\u0301".encode()), s(b" " + "c\u0327".encode())]))
    e.append(entry("bmp-symbols", "BMP symbols and box drawing",
                   [s("★☆☀☂╭─╮│╰─╯".encode())]))

    # --- 12. realistic program output ---------------------------------------------
    e.append(entry("real-ls-colors", "ls --color output",
                   [s(b"\x1b[0m\x1b[01;34mDocuments\x1b[0m  \x1b[01;34mDownloads\x1b[0m  "
                      b"\x1b[01;32mrun.sh\x1b[0m  \x1b[01;31merror.log\x1b[0m\r\n")]))
    e.append(entry("real-git-diff", "git diff colored hunks",
                   [s(b"\x1b[1mdiff --git a/x b/x\x1b[0m\r\n"
                      b"\x1b[32m+added line\x1b[0m\r\n\x1b[31m-removed line\x1b[0m\r\n")]))
    e.append(entry("real-progress-carriage", "wget-style progress with CR and EL",
                   [s(b"\r" + b"".join(b"\x1b[K%3d%% [%s]" % (p, b"#" * (p // 10))
                                       for p in range(0, 101, 20)))]))
    e.append(entry("real-htop-like", "htop-like bar and column layout",
                   [s(csi("H") + csi("2J")
                      + b"\x1b[7m  1  \x1b[0m\x1b[32m||||||\x1b[0m 45.2%\r\n"
                      + b"\x1b[7m  2  \x1b[0m\x1b[31m||||||||||||\x1b[0m 88.7%\r\n")]))
    e.append(entry("real-python-traceback", "python traceback with caret line",
                   [s(b"Traceback (most recent call last):\r\n"
                      b"  File \"x.py\", line 1\r\n    raise ValueError()\r\n"
                      b"ValueError\r\n")]))
    e.append(entry("real-readline-prompt", "bash readline prompt editing",
                   [s(b"user@host:~$ echo hi\r\n"), s(b"hi\r\n"),
                    s(b"user@host:~$ \x1b[K")]))

    return e


def main() -> None:
    items = entries()
    ids = [i["id"] for i in items]
    assert len(ids) == len(set(ids)), "corpus ids must be unique"

    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", encoding="utf-8") as fh:
        for item in items:
            record = {
                "id": item["id"],
                "desc": item["desc"],
                "cols": item["cols"],
                "rows": item["rows"],
                "transcript": item["transcript"],
                "steps": [
                    {"send": step["send"].hex()} if "send" in step
                    else {"resize": step["resize"]}
                    for step in item["steps"]
                ],
            }
            fh.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")

    total_bytes = sum(
        len(step["send"]) for item in items for step in item["steps"] if "send" in step
    )
    total_steps = sum(len(item["steps"]) for item in items)
    print(f"wrote {len(items)} sequences / {total_steps} steps / {total_bytes} bytes -> {OUT}")


if __name__ == "__main__":
    main()