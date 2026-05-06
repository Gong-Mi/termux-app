#[cfg(test)]
mod tests {
    use crate::terminal::screen::Screen;

    #[test]
    fn prove_reflow_stacking_bug() {
        // Simulate an 80x24 terminal
        let mut s = Screen::new(80, 24, 100);

        // Simulate prompt "~$" and user cursor at index 3
        let prompt = "~$ ";
        let mut col = 0;
        for c in prompt.chars() {
            s.get_row_mut(0).set_char(col, c as u32, 0);
            col += 1;
        }

        // Cursor is placed right after prompt
        let cx = 3;
        let cy = 0;

        // Before reflow, line wrap is false
        assert_eq!(s.get_row(0).line_wrap, false);

        // Trigger reflow: simulate keyboard popup narrowing width to 40
        let (_new_cx, _new_cy) = s.resize_with_reflow(40, 12, 0, cx, cy);

        // The prompt is 3 characters. Even in a 40-col screen, it easily fits.
        // It SHOULD NOT wrap.
        // However, because the Rust loop processes all 80 characters (including trailing spaces)
        // instead of breaking early like Java's `justToCursor`, it will trigger a wrap.

        let has_bug = s.get_row(0).line_wrap;

        assert!(
            !has_bug,
            "BUG PROVEN: The screen incorrectly wrapped the cursor row because it processed invisible trailing spaces. This pushes content down, causing the 'stacking' effect."
        );
    }
}
