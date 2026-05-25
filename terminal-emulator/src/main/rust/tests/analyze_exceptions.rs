// tests/analyze_exceptions.rs
// This integration test runs only on Android devices. On other platforms it is skipped.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

/// Core logic: open the SQLite DB and print glyph exception information.
fn run_analysis(db_path: &Path) -> Result<()> {
    let conn = Connection::open(db_path)?;

    let mut stmt = conn.prepare(
        "SELECT cp, actual_width, expected_width, direction, category FROM glyph_exceptions LIMIT 100",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, f32>(1)?,
            row.get::<_, f32>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;

    println!(
        "{:<8} | {:<4} | {:<10} | {:<10} | {:<6} | {:<10}",
        "CP(Hex)", "Char", "Actual(px)", "Expect(px)", "Units", "Category"
    );
    println!("{:-<60}", "");

    let base_w = 11.0f32; // "sans-serif" baseline width

    for row in rows {
        let (cp, actual_w, expected_w, _direction, category) = row?;
        let ch = std::char::from_u32(cp).unwrap_or(' ');
        let units = actual_w / base_w;
        println!(
            "U+{:04X}   | {:<4} | {:<10.2} | {:<10.2} | {:<6.2} | {:<10}",
            cp, ch, actual_w, expected_w, units, category
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // This test only runs on Android devices where the DB is available.
    #[test]
    #[cfg(target_os = "android")]
    fn analyze_exceptions_android() {
        // Path is relative to the project root; adjust if needed for CI.
        let db_path = PathBuf::from(
            "terminal-emulator/src/main/rust/tests/calibration_production.db",
        );
        // In case the file is missing, the test will fail with a clear error.
        run_analysis(&db_path).expect("Failed to analyze glyph exceptions on Android");
    }

    // On non-Android platforms we simply skip the test.
    #[test]
    #[cfg(not(target_os = "android"))]
    fn analyze_exceptions_skip() {
        println!(
            "Skipping analyze_exceptions test: only relevant on Android local environment."
        );
    }
}
