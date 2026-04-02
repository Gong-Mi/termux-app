package com.termux.terminal;

import android.util.Base64;

import java.util.ArrayList;
import java.util.List;
import java.util.Random;

/** "ESC ]" is the Operating System Command. */
public class OperatingSystemControlTest extends TerminalTestCase {

	public void testSetTitle() throws Exception {
		List<ChangedTitle> expectedTitleChanges = new ArrayList<>();

		withTerminalSized(10, 10);
		enterString("\033]0;Hello, world\007");
		assertEquals("Hello, world", mTerminal.getTitle());
		expectedTitleChanges.add(new ChangedTitle(null, "Hello, world"));
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		enterString("\033]0;Goodbye, world\007");
		assertEquals("Goodbye, world", mTerminal.getTitle());
		expectedTitleChanges.add(new ChangedTitle("Hello, world", "Goodbye, world"));
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		enterString("\033]0;Goodbye, \u00F1 world\007");
		assertEquals("Goodbye, \uu00F1 world", mTerminal.getTitle());
		expectedTitleChanges.add(new ChangedTitle("Goodbye, world", "Goodbye, \uu00F1 world"));
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		// 2 should work as well (0 sets both title and icon).
		enterString("\033]2;Updated\007");
		assertEquals("Updated", mTerminal.getTitle());
		expectedTitleChanges.add(new ChangedTitle("Goodbye, \uu00F1 world", "Updated"));
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		enterString("\033[22;0t");
		enterString("\033]0;FIRST\007");
		expectedTitleChanges.add(new ChangedTitle("Updated", "FIRST"));
		assertEquals("FIRST", mTerminal.getTitle());
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		enterString("\033[22;0t");
		enterString("\033]0;SECOND\007");
		assertEquals("SECOND", mTerminal.getTitle());

		expectedTitleChanges.add(new ChangedTitle("FIRST", "SECOND"));
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		enterString("\033[23;0t");
		assertEquals("FIRST", mTerminal.getTitle());

		expectedTitleChanges.add(new ChangedTitle("SECOND", "FIRST"));
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		enterString("\033[23;0t");
		expectedTitleChanges.add(new ChangedTitle("FIRST", "Updated"));
		assertEquals(expectedTitleChanges, mOutput.titleChanges);

		enterString("\033[22;0t");
		enterString("\033[22;0t");
		enterString("\033[22;0t");
		// Popping to same title should not cause changes.
		enterString("\033[23;0t");
		enterString("\033[23;0t");
		enterString("\033[23;0t");
		assertEquals(expectedTitleChanges, mOutput.titleChanges);
	}

	public void testTitleStack() throws Exception {
		// echo -ne '\e]0;BEFORE\007' # set title
		// echo -ne '\e[22t' # push to stack
		// echo -ne '\e]0;AFTER\007' # set new title
		// echo -ne '\e[23t' # retrieve from stack

		withTerminalSized(10, 10);
		enterString("\033]0;InitialTitle\007");
		assertEquals("InitialTitle", mTerminal.getTitle());
		enterString("\033[22t");
		assertEquals("InitialTitle", mTerminal.getTitle());
		enterString("\033]0;UpdatedTitle\007");
		assertEquals("UpdatedTitle", mTerminal.getTitle());
		enterString("\033[23t");
		assertEquals("InitialTitle", mTerminal.getTitle());
		enterString("\033[23t\033[23t\033[23t");
		assertEquals("InitialTitle", mTerminal.getTitle());
	}

	public void testSetColor() throws Exception {
		// "OSC 4; $INDEX; $COLORSPEC BEL" => Change color $INDEX to the color specified by $COLORSPEC.
		withTerminalSized(4, 4).enterString("\033]4;5;#00FF00\007");
		assertEquals(Integer.toHexString(0xFF00FF00), Integer.toHexString(mTerminal.mColors.mCurrentColors[5]));
		enterString("\033]4;5;#00FFAB\007");
		assertEquals(mTerminal.mColors.mCurrentColors[5], 0xFF00FFAB);
		enterString("\033]4;255;#ABFFAB\007");
		assertEquals(mTerminal.mColors.mCurrentColors[255], 0xFFABFFAB);
		// Two indexed colors at once:
		enterString("\033]4;7;#00FF00;8;#0000FF\007");
		assertEquals(mTerminal.mColors.mCurrentColors[7], 0xFF00FF00);
		assertEquals(mTerminal.mColors.mCurrentColors[8], 0xFF0000FF);
	}

	public void disabledTestSetClipboard() {
		// Cannot run this as a unit test since Base64 is a android.util class.
		enterString("\033]52;c;" + Base64.encodeToString("Hello, world".getBytes(), 0) + "\007");
	}

}
