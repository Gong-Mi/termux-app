//! Differential oracle gate: the Rust engine against the pinned upstream implementation.
//!
//! Why this exists
//! ---------------
//! Every other test in this crate asserts expectations written by the same people who wrote
//! the engine. That kind of test can catch a typo, but it can never catch a *misunderstanding*
//! of terminal semantics, because the misunderstanding is baked into the expectation as well.
//! This gate removes authorship from the expectations: the expected values come from upstream
//! Termux's own `TerminalEmulator` at the pinned baseline commit
//! (`e634d8f981f48b6b89202cf0e04533f0889e03b3`, v0.119.0-beta.3), dumped into
//! `corpus/golden/upstream-e634d8f.jsonl` by `tools/oracle/run.sh`.
//!
//! What is compared
//! ----------------
//! Both implementations are fed the same byte sequences from `corpus/sequences/seed.jsonl` and
//! compared in **column space** -- per column: the code point whose glyph starts there, a
//! continuation marker for the second half of a wide character, and the style bits. Column
//! space is the level the two implementations must agree on, because it is what the renderer
//! draws and what wrapping is computed from. The two sides store rows differently (upstream
//! packs a width-2 character into a single array slot; this engine stores one slot per column),
//! so comparing storage layout directly would report representation noise instead of behaviour.
//!
//! Coverage, not pass/fail alone
//! ----------------------------
//! A gate that silently compares nothing is worse than no gate, so this test asserts that it
//! actually ran: every corpus sequence must appear in the golden file, and the number of
//! compared cells must exceed a floor. Any drop in coverage fails the gate.
//!
//! Ratchet
//! -------
//! The port is not expected to match upstream on day one, so known divergences are recorded in
//! `corpus/oracle_baseline.json` as `"<sequence>": {"cells": N, "state": M}` -- the number of
//! differing cells and the number of per-step state mismatches (cursor, title, alternate buffer)
//! that sequence is allowed to carry. Both counters are ratchets: the gate fails when a sequence
//! introduces a *new* divergence, or makes a recorded one worse in either counter, and reports
//! when the baseline can shrink. State-only divergences (0 cells) are covered as well, so a
//! cursor or title mismatch cannot hide behind a baseline entry that only counts cells. A
//! sequence that starts matching upstream is reported too: keep a stale entry and the same
//! sequence could later regress all the way back to the recorded numbers unnoticed. Run with
//! `ORACLE_MODE=report` to list all divergences for adoption without failing,
//! `ORACLE_WRITE_BASELINE=1` to write the current state as the new baseline.
//!
//! Environment
//! -----------
//!   ORACLE_GOLDEN          golden file (default: corpus/golden/upstream-e634d8f.jsonl)
//!   ORACLE_CORPUS          corpus file (default: corpus/sequences/seed.jsonl)
//!   ORACLE_BASELINE        baseline file (default: corpus/oracle_baseline.json)
//!   ORACLE_DIFF_REPORT     report path (default: target/oracle_diff_report.json)
//!   ORACLE_MODE            gate (default) | report
//!   ORACLE_MAX_DIFFS_SHOWN how many cell-level diffs to keep per sequence (default 12)

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use termux_rust::terminal::screen::TerminalRow;
use termux_rust::wcwidth::wcwidth;
use termux_rust::TerminalEngine;

/// Cell value used when nothing starts at a column.
const BLANK_CELL: i32 = ' ' as i32;
/// Cell value marking the second (or later) column of a wide character.
const CONTINUATION_CELL: i32 = -1;
/// Cell comparison is only meaningful over the whole screen: a gate that compares a handful of
/// cells is green for the wrong reason. The floor is deliberately below the current corpus so
/// ordinary corpus growth does not trip it, but a broken harness does.
const MIN_COMPARED_CELLS: u64 = 100_000;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .expect("repository root is four levels above the crate manifest")
        .to_path_buf()
}

fn env_path(key: &str, default: PathBuf) -> PathBuf {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => default,
    }
}

fn read_json_lines(path: &Path) -> Vec<Value> {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("{}:{} is not valid JSON: {e}", path.display(), index + 1)
        });
        out.push(value);
    }
    out
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    assert!(
        hex.len().is_multiple_of(2),
        "hex payload must have an even length, got {}",
        hex.len()
    );
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex payload"))
        .collect()
}

fn decode_rle(rle: &Value) -> Vec<i64> {
    let items = rle.as_array().expect("rle arrays");
    assert!(
        items.len().is_multiple_of(2),
        "rle array must be [count, value, ...], got {} entries",
        items.len()
    );
    let mut out = Vec::new();
    for pair in items.chunks(2) {
        let count = pair[0].as_i64().expect("rle count");
        let value = pair[1].as_i64().expect("rle value");
        assert!(count >= 0, "rle count must not be negative");
        for _ in 0..count {
            out.push(value);
        }
    }
    out
}

/// Project one row of this engine into column space.
///
/// Continuation columns are derived from the width of the base character and never read from
/// storage, so it does not matter which filler value a wide character leaves behind.
fn project_row(row: &TerminalRow, cols: usize) -> (Vec<i64>, usize) {
    let mut cells = vec![BLANK_CELL as i64; cols];
    let mut zero_width = 0usize;
    let mut col = 0usize;
    while col < cols {
        let ch = match row.text.get(col) {
            Some(ch) => *ch,
            None => break,
        };
        let code_point = ch as u32;
        if code_point == 0 {
            col += 1;
            continue;
        }
        let width = wcwidth(code_point);
        if width == 0 {
            zero_width += 1;
            col += 1;
            continue;
        }
        cells[col] = code_point as i64;
        for offset in 1..width {
            if col + offset < cols {
                cells[col + offset] = CONTINUATION_CELL as i64;
            }
        }
        col += width;
    }
    (cells, zero_width)
}

/// Transcript digest built exactly like the reference side builds it: rows above the visible
/// screen, continuation columns dropped, blanks as spaces, trailing spaces trimmed, joined by
/// newlines.
fn transcript_hash(engine: &TerminalEngine, cols: usize) -> String {
    let screen = engine.state.get_current_screen();
    let mut text = String::new();
    for internal in 0..screen.first_row.min(screen.buffer.len()) {
        let row = &screen.buffer[internal];
        let (cells, _) = project_row(row, cols);
        let mut line = String::new();
        for cell in cells {
            if cell == CONTINUATION_CELL as i64 {
                continue;
            }
            line.push(if cell == 0 { ' ' } else { char::from_u32(cell as u32).unwrap_or(' ') });
        }
        let trimmed = line.trim_end();
        text.push_str(trimmed);
        text.push('\n');
    }
    format!("{:x}", md5::compute(text.as_bytes()))
}

struct Snapshot {
    cols: usize,
    cells: Vec<i64>,
    styles: Vec<i64>,
    zero_width: Vec<usize>,
    cursor: (i64, i64),
    title: String,
    alt: bool,
    transcript_rows: u64,
    first_row: u64,
    transcript_hash: String,
}

#[allow(clippy::needless_range_loop)] // cell and style arrays are indexed by the same column
fn take_snapshot(engine: &TerminalEngine) -> Snapshot {
    let cols = engine.state.cols.max(0) as usize;
    let rows = engine.state.rows.max(0) as usize;
    let screen = engine.state.get_current_screen();
    let mut cells = Vec::with_capacity(cols * rows);
    let mut styles = Vec::with_capacity(cols * rows);
    let mut zero_width = Vec::with_capacity(rows);
    for row_index in 0..rows {
        let row = screen.get_row(row_index as i32);
        let (row_cells, row_zero_width) = project_row(row, cols);
        zero_width.push(row_zero_width);
        for col in 0..cols {
            cells.push(row_cells[col]);
            styles.push(row.styles.get(col).copied().unwrap_or(0) as i64);
        }
    }
    Snapshot {
        cols,
        cells,
        styles,
        zero_width,
        cursor: (engine.state.cursor.x as i64, engine.state.cursor.y as i64),
        title: engine.state.title.clone().unwrap_or_default(),
        alt: engine.state.use_alternate_buffer,
        transcript_rows: screen.active_transcript_rows as u64,
        first_row: screen.first_row as u64,
        transcript_hash: transcript_hash(engine, cols),
    }
}

#[derive(Default)]
struct EntryDiff {
    hard_cells: u64,
    hard_state: Vec<String>,
    soft: Vec<String>,
    examples: Vec<Value>,
}

impl EntryDiff {
    fn is_clean(&self) -> bool {
        self.hard_cells == 0 && self.hard_state.is_empty()
    }
}

/// One recorded divergence: how many cells may differ from upstream, and how many per-step state
/// mismatches (cursor, title, alternate buffer) are tolerated alongside them. Both numbers are
/// ratchets -- they may only shrink, and anything not in the baseline fails the gate.
#[derive(Clone, Copy, Default)]
struct KnownDivergence {
    cells: u64,
    state: u64,
}

/// Read the ratchet baseline. Entries are `"<sequence>": {"cells": N, "state": M}`; a bare number
/// is still read as `{"cells": N, "state": 0}` so an older baseline keeps working.
fn read_baseline(path: &Path) -> BTreeMap<String, KnownDivergence> {
    if !path.exists() {
        return BTreeMap::new();
    }
    let text = fs::read_to_string(path).expect("read baseline");
    let raw: BTreeMap<String, Value> = match serde_json::from_str(&text) {
        Ok(raw) => raw,
        Err(e) => panic!("baseline {} is not valid JSON: {e}", path.display()),
    };
    raw.into_iter()
        .map(|(id, value)| {
            let entry = match value {
                Value::Number(number) => KnownDivergence {
                    cells: number.as_u64().unwrap_or(0),
                    state: 0,
                },
                Value::Object(map) => KnownDivergence {
                    cells: map.get("cells").and_then(Value::as_u64).unwrap_or(0),
                    state: map.get("state").and_then(Value::as_u64).unwrap_or(0),
                },
                other => panic!("baseline entry {id} must be a number or a cells/state object, got {other}"),
            };
            (id, entry)
        })
        .collect()
}

/// Write the divergences observed by this run in the shape `read_baseline` accepts.
fn write_baseline(path: &Path, entries: &BTreeMap<String, KnownDivergence>) {
    let object: serde_json::Map<String, Value> = entries
        .iter()
        .map(|(id, entry)| {
            (
                id.clone(),
                json!({"cells": entry.cells, "state": entry.state}),
            )
        })
        .collect();
    let text = serde_json::to_string_pretty(&Value::Object(object)).expect("serialize baseline");
    fs::write(path, format!("{text}\n"))
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
}

#[allow(clippy::needless_range_loop)] // row/column indices are used in the diff report
fn compare_snapshot(
    id: &str,
    step: usize,
    golden: &Value,
    engine: &TerminalEngine,
    max_examples: usize,
    diff: &mut EntryDiff,
) {
    let actual = take_snapshot(engine);
    let golden_cols = golden["cols"].as_u64().unwrap_or(0) as usize;
    let golden_rows = golden["rows"].as_u64().unwrap_or(0) as usize;

    if golden_cols != actual.cols || golden_rows != (actual.cells.len() / actual.cols.max(1)) {
        diff.hard_state.push(format!(
            "step {step}: geometry differs, golden {golden_cols}x{golden_rows} vs engine {}x{}",
            actual.cols,
            actual.cells.len() / actual.cols.max(1)
        ));
        return;
    }

    let golden_screen = golden["screen"].as_array().expect("golden screen array");
    if golden_screen.len() != actual.zero_width.len() {
        diff.hard_state.push(format!(
            "step {step}: golden has {} rows, engine has {}",
            golden_screen.len(),
            actual.zero_width.len()
        ));
        return;
    }

    for (row, golden_row) in golden_screen.iter().enumerate() {
        let golden_cells = decode_rle(&golden_row["cp"]);
        let golden_styles = decode_rle(&golden_row["st"]);
        if golden_cells.len() != actual.cols {
            diff.hard_state.push(format!(
                "step {step} row {row}: golden row has {} cells, expected {}",
                golden_cells.len(),
                actual.cols
            ));
            continue;
        }
        let golden_zero_width = golden_row["zw"].as_u64().unwrap_or(0) as usize;
        if golden_zero_width != actual.zero_width[row] {
            diff.soft.push(format!(
                "step {step} row {row}: zero-width count {} vs {}",
                golden_zero_width, actual.zero_width[row]
            ));
        }
        for col in 0..actual.cols {
            let index = row * actual.cols + col;
            let golden_cell = golden_cells[col];
            let actual_cell = actual.cells[index];
            let golden_style = golden_styles[col];
            let actual_style = actual.styles[index];
            if golden_cell != actual_cell || golden_style != actual_style {
                diff.hard_cells += 1;
                if diff.examples.len() < max_examples {
                    diff.examples.push(json!({
                        "id": id,
                        "step": step,
                        "row": row,
                        "col": col,
                        "golden_cp": golden_cell,
                        "engine_cp": actual_cell,
                        "golden_style": format!("0x{:016x}", golden_style as u64),
                        "engine_style": format!("0x{:016x}", actual_style as u64),
                    }));
                }
            }
        }
    }

    let golden_cursor = golden["cursor"].as_array().expect("golden cursor");
    let golden_cursor = (
        golden_cursor[0].as_i64().unwrap_or(-1),
        golden_cursor[1].as_i64().unwrap_or(-1),
    );
    if golden_cursor != actual.cursor {
        diff.hard_state.push(format!(
            "step {step}: cursor {:?} vs {:?}",
            golden_cursor, actual.cursor
        ));
    }

    let golden_title = golden["title"].as_str().unwrap_or("");
    if golden_title != actual.title {
        diff.hard_state
            .push(format!("step {step}: title {golden_title:?} vs {:?}", actual.title));
    }

    let golden_alt = golden["alt"].as_bool().unwrap_or(false);
    if golden_alt != actual.alt {
        diff.hard_state
            .push(format!("step {step}: alternate buffer {golden_alt} vs {}", actual.alt));
    }

    let golden_transcript = golden["transcript_rows"].as_u64().unwrap_or(0);
    if golden_transcript != actual.transcript_rows {
        diff.soft.push(format!(
            "step {step}: transcript rows {golden_transcript} vs {}",
            actual.transcript_rows
        ));
    }
    let golden_first = golden["first_row"].as_u64().unwrap_or(0);
    if golden_first != actual.first_row {
        diff.soft.push(format!(
            "step {step}: first row {golden_first} vs {}",
            actual.first_row
        ));
    }
    let golden_hash = golden["transcript_hash"].as_str().unwrap_or("");
    if golden_hash != actual.transcript_hash {
        diff.soft.push(format!("step {step}: transcript hash differs (soft)"));
    }
}

#[test]
fn gate_self_check_projects_wide_characters() {
    // A gate whose own projection is wrong would report confident nonsense. Guard the rules the
    // comparison depends on, using rows shaped the way the engine actually stores them:
    // one slot per column, with the second column of a wide character holding the '\0' filler
    // (`terminal/handlers/print.rs`). The continuation marker must come from the base
    // character's width, never from that filler value.
    let row = TerminalRow {
        text: vec!['中', '\0', 'a', 'b', ' ', ' '],
        styles: vec![7; 6],
        line_wrap: false,
    };
    let (cells, zero_width) = project_row(&row, 6);
    assert_eq!(cells[0], '中' as i64, "base column keeps the code point");
    assert_eq!(cells[1], CONTINUATION_CELL as i64, "second column is a continuation");
    assert_eq!(cells[2], 'a' as i64, "next base character starts after the wide glyph");
    assert_eq!(cells[3], 'b' as i64, "narrow characters keep their own column");
    assert_eq!(
        zero_width, 0,
        "the wide-character filler must not be counted as a zero-width character"
    );

    // Defensive path: the engine drops width-0 characters before they reach a row, so a row that
    // carries one is out of spec -- the projection must still skip it instead of turning it into
    // a cell, and must count it.
    let row = TerminalRow {
        text: vec!['a', '\u{0301}', 'b', ' ', ' '],
        styles: vec![7; 5],
        line_wrap: false,
    };
    let (cells, zero_width) = project_row(&row, 5);
    assert_eq!(cells[0], 'a' as i64, "base character keeps its column");
    assert_eq!(cells[1], BLANK_CELL as i64, "a combining mark never becomes a cell");
    assert_eq!(zero_width, 1, "the combining mark must be counted");
}

#[test]
fn oracle_diff_matches_upstream_reference() {
    let root = repo_root();
    let corpus_path = env_path("ORACLE_CORPUS", root.join("corpus/sequences/seed.jsonl"));
    let golden_path = env_path(
        "ORACLE_GOLDEN",
        root.join("corpus/golden/upstream-e634d8f.jsonl"),
    );
    let baseline_path = env_path("ORACLE_BASELINE", root.join("corpus/oracle_baseline.json"));
    let report_path = env_path(
        "ORACLE_DIFF_REPORT",
        root.join("terminal-emulator/src/main/rust/target/oracle_diff_report.json"),
    );
    let mode = std::env::var("ORACLE_MODE").unwrap_or_else(|_| "gate".to_string());
    let max_examples: usize = std::env::var("ORACLE_MAX_DIFFS_SHOWN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);

    assert!(
        golden_path.exists(),
        "golden file {} is missing; generate it with tools/oracle/run.sh",
        golden_path.display()
    );

    let corpus = read_json_lines(&corpus_path);
    let golden = read_json_lines(&golden_path);
    // The corpus carries the inputs, the golden file carries the reference snapshots. They are
    // paired by sequence id and then by step order inside the sequence; every snapshot records
    // its own step index and kind, which the loop below cross-checks so a reordered or truncated
    // golden file cannot silently shift the comparison.
    let corpus_by_id: BTreeMap<&str, &Value> = corpus
        .iter()
        .map(|entry| (entry["id"].as_str().expect("corpus id"), entry))
        .collect();
    let corpus_ids: BTreeSet<String> = corpus
        .iter()
        .map(|entry| entry["id"].as_str().expect("corpus id").to_string())
        .collect();
    let golden_ids: BTreeSet<String> = golden
        .iter()
        .map(|entry| entry["id"].as_str().expect("golden id").to_string())
        .collect();

    // Coverage: the oracle must have exercised every sequence, otherwise a "green" run would
    // just mean the comparison never looked at the interesting cases.
    let missing: Vec<&String> = corpus_ids.difference(&golden_ids).collect();
    assert!(
        missing.is_empty(),
        "{} corpus sequences were never dumped by the oracle: {missing:?}",
        missing.len()
    );
    let extra: Vec<&String> = golden_ids.difference(&corpus_ids).collect();
    assert!(
        extra.is_empty(),
        "golden file contains sequences that no longer exist in the corpus: {extra:?} \
         (regenerate with tools/oracle/run.sh)"
    );

    let baseline = read_baseline(&baseline_path);

    let mut compared_cells: u64 = 0;
    let mut report_entries: Vec<Value> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut improvements: Vec<String> = Vec::new();
    let mut current_baseline: BTreeMap<String, KnownDivergence> = BTreeMap::new();
    let mut clean_sequences = 0usize;

    for golden_entry in &golden {
        let id = golden_entry["id"].as_str().expect("golden id").to_string();
        let corpus_entry = corpus_by_id
            .get(id.as_str())
            .copied()
            .unwrap_or_else(|| panic!("corpus has no sequence {id}"));
        let input_steps = corpus_entry["steps"].as_array().expect("corpus steps");
        let snapshots = golden_entry["snapshots"]
            .as_array()
            .expect("golden snapshots");
        assert_eq!(
            input_steps.len(),
            snapshots.len(),
            "sequence {id}: golden has {} snapshots for {} corpus steps",
            snapshots.len(),
            input_steps.len()
        );
        let cols = golden_entry["cols"].as_u64().expect("cols") as i32;
        let rows = golden_entry["rows"].as_u64().expect("rows") as i32;
        let transcript = golden_entry["transcript"].as_u64().expect("transcript") as i32;

        let mut engine = TerminalEngine::new(1, cols, rows, transcript, 10, 20);
        let mut diff = EntryDiff::default();

        for (step_index, (step, golden_snapshot)) in
            input_steps.iter().zip(snapshots.iter()).enumerate()
        {
            assert_eq!(
                golden_snapshot["step"].as_u64(),
                Some(step_index as u64),
                "sequence {id}: golden snapshot order does not match the corpus step order"
            );
            let kind = golden_snapshot["kind"].as_str().unwrap_or("");
            if let Some(hex) = step.get("send").and_then(|v| v.as_str()) {
                assert_eq!(
                    kind, "send",
                    "sequence {id} step {step_index}: snapshot kind {kind:?} does not match a send step"
                );
                engine.process_bytes(&hex_to_bytes(hex));
            } else if let Some(dims) = step.get("resize").and_then(|v| v.as_array()) {
                assert_eq!(
                    kind, "resize",
                    "sequence {id} step {step_index}: snapshot kind {kind:?} does not match a resize step"
                );
                let new_cols = dims[0].as_i64().expect("resize cols") as i32;
                let new_rows = dims[1].as_i64().expect("resize rows") as i32;
                engine.state.resize(new_cols, new_rows);
            } else {
                panic!(
                    "sequence {id} step {step_index}: corpus step is neither send nor resize: {step}"
                );
            }
            compare_snapshot(&id, step_index, golden_snapshot, &engine, max_examples, &mut diff);
            compared_cells += golden_snapshot["cells"].as_u64().unwrap_or(0);
        }

        if diff.is_clean() {
            clean_sequences += 1;
        }

        let state_count = diff.hard_state.len() as u64;

        if !diff.is_clean() {
            current_baseline.insert(
                id.clone(),
                KnownDivergence {
                    cells: diff.hard_cells,
                    state: state_count,
                },
            );
        }

        // 已登记的分歧：只许变小；序列修到与上游一致时提示把登记删掉，否则过期的登记数
        // 会让同一条序列将来再退化时被静默放行。
        if let Some(known) = baseline.get(&id) {
            if diff.is_clean() {
                improvements.push(format!(
                    "{id}: now matches upstream, drop the baseline entry ({} cells, {} state)",
                    known.cells, known.state
                ));
            } else if known.cells >= diff.hard_cells && known.state >= state_count {
                if known.cells > diff.hard_cells || known.state > state_count {
                    improvements.push(format!(
                        "{id}: cells {} -> {}, state {} -> {}",
                        known.cells, diff.hard_cells, known.state, state_count
                    ));
                }
            } else {
                failures.push(format!(
                    "{id}: known divergence got worse, cells {} -> {}, state {} -> {}",
                    known.cells, diff.hard_cells, known.state, state_count
                ));
            }
        } else if !diff.is_clean() {
            failures.push(format!(
                "{id}: new divergence, {} cells, {} state mismatches{}{}",
                diff.hard_cells,
                state_count,
                if diff.hard_state.is_empty() { "" } else { ": " },
                diff.hard_state.join(" | ")
            ));
        }

        report_entries.push(json!({
            "id": id,
            "desc": corpus_entry["desc"],
            "status": if diff.is_clean() { "match" } else { "divergent" },
            "hard_diff_cells": diff.hard_cells,
            "hard_state": diff.hard_state,
            "soft_diffs": diff.soft,
            "examples": diff.examples,
        }));
    }

    assert!(
        compared_cells >= MIN_COMPARED_CELLS,
        "coverage guard: only {compared_cells} cells compared, floor is {MIN_COMPARED_CELLS}; \
         a gate that compares almost nothing is green for the wrong reason"
    );

    let report = json!({
        "upstream_pin": "e634d8f981f48b6b89202cf0e04533f0889e03b3",
        "sequences": report_entries.len(),
        "clean_sequences": clean_sequences,
        "compared_cells": compared_cells,
        "mode": mode,
        "entries": report_entries,
    });
    if let Some(parent) = report_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&report_path, serde_json::to_string_pretty(&report).expect("serialize report"))
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", report_path.display()));

    println!(
        "oracle-diff: {} sequences ({} matching upstream), {} cells compared, report {}",
        report_entries.len(),
        clean_sequences,
        compared_cells,
        report_path.display()
    );
    for failure in failures.iter().take(30) {
        println!("oracle-diff: DIVERGENCE {failure}");
    }
    if failures.len() > 30 {
        println!("oracle-diff: ... {} more divergences in the report", failures.len() - 30);
    }
    for improvement in &improvements {
        println!("oracle-diff: baseline can shrink: {improvement}");
    }

    if std::env::var("ORACLE_WRITE_BASELINE").is_ok() {
        write_baseline(&baseline_path, &current_baseline);
        println!(
            "oracle-diff: wrote baseline with {} divergent sequences to {}",
            current_baseline.len(),
            baseline_path.display()
        );
        return;
    }

    if mode == "report" {
        println!(
            "oracle-diff: report mode, {} divergences are informational",
            failures.len()
        );
        return;
    }

    assert!(
        failures.is_empty(),
        "{} sequences diverge from the upstream reference:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}