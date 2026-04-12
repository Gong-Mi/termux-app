use rusqlite::{Connection};
use std::path::Path;

fn main() {
    let db_path = Path::new("terminal-emulator/src/main/rust/tests/calibration_production.db");
    let conn = Connection::open(db_path).unwrap();

    let mut stmt = conn.prepare(
        "SELECT cp, actual_width, expected_width, direction, category FROM glyph_exceptions LIMIT 100"
    ).unwrap();

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, f32>(1)?,
            row.get::<_, i32>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    }).unwrap();

    println!("{:<8} | {:<4} | {:<10} | {:<10} | {:<6} | {:<10}", "CP(Hex)", "Char", "Actual(px)", "Expect(un)", "Units", "Category");
    println!("{:-<60}", "");

    let base_w = 11.0f32; // "sans-serif" 的基准宽度

    for row in rows {
        let (cp, actual_w, expected_w, direction, category) = row.unwrap();
        let ch = std::char::from_u32(cp).unwrap_or(' ');
        let units = actual_w / base_w;
        
        println!(
            "U+{:04X}   | {:<4} | {:<10.2} | {:<10} | {:<6.2} | {:<10}",
            cp, ch, actual_w, expected_w, units, category
        );
    }
}
