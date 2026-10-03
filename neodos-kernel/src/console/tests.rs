//! ANSI console tests.

use core::sync::atomic::Ordering;
use super::*;
use super::ansi::*;

// ── ANSI tests ────────────────────────────────────────────────────────────
pub fn register_ansi_tests() {
    use crate::test_case;
    use crate::test_eq;
    use crate::test_true;

    test_case!("ansi_color_foreground", {
        let saved_fg = ANSI_FG.load(Ordering::Relaxed);
        let saved_bg = ANSI_BG.load(Ordering::Relaxed);
        let saved_bold = ANSI_BOLD.load(Ordering::Relaxed);

        // ESC[31m — red foreground
        print_str("\x1b[31m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 1);

        // ESC[41m — red background
        print_str("\x1b[41m");
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 1);

        // ESC[1m — bold
        print_str("\x1b[1m");
        test_true!(ANSI_BOLD.load(Ordering::Relaxed));

        // ESC[0m — reset
        print_str("\x1b[0m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 7);
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 0);
        test_eq!(ANSI_BOLD.load(Ordering::Relaxed), false);

        // ESC[91m — bright red foreground
        print_str("\x1b[91m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 9);

        // ESC[107m — bright white background
        print_str("\x1b[107m");
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 15);

        // ESC[1;32m — bold + green
        print_str("\x1b[0m");
        print_str("\x1b[1;32m");
        test_true!(ANSI_BOLD.load(Ordering::Relaxed));
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 2);

        // ESC[39m — default fg
        print_str("\x1b[39m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), 7);

        // ESC[49m — default bg
        print_str("\x1b[49m");
        test_eq!(ANSI_BG.load(Ordering::Relaxed), 0);

        // Restore
        ANSI_FG.store(saved_fg, Ordering::Relaxed);
        ANSI_BG.store(saved_bg, Ordering::Relaxed);
        ANSI_BOLD.store(saved_bold, Ordering::Relaxed);
    });

    test_case!("ansi_cursor_position", {
        let saved_row = ROW.load(Ordering::SeqCst);
        let saved_col = COL.load(Ordering::SeqCst);

        // ESC[10;20H — cursor to row 10, col 20
        print_str("\x1b[10;20H");
        test_eq!(ROW.load(Ordering::SeqCst), 9);
        test_eq!(COL.load(Ordering::SeqCst), 19);

        // ESC[H — cursor home
        print_str("\x1b[H");
        test_eq!(ROW.load(Ordering::SeqCst), 0);
        test_eq!(COL.load(Ordering::SeqCst), 0);

        // ESC[1;1H — explicit home
        print_str("\x1b[1;1H");
        test_eq!(ROW.load(Ordering::SeqCst), 0);
        test_eq!(COL.load(Ordering::SeqCst), 0);

        // ESC[f — alternative home
        print_str("\x1b[5;15f");
        test_eq!(ROW.load(Ordering::SeqCst), 4);
        test_eq!(COL.load(Ordering::SeqCst), 14);

        // Restore
        ROW.store(saved_row, Ordering::SeqCst);
        COL.store(saved_col, Ordering::SeqCst);
    });

    test_case!("ansi_clear_screen", {
        let saved_row = ROW.load(Ordering::SeqCst);
        let saved_col = COL.load(Ordering::SeqCst);

        // Move cursor to known position
        print_str("\x1b[5;10H");
        test_eq!(ROW.load(Ordering::SeqCst), 4);
        test_eq!(COL.load(Ordering::SeqCst), 9);

        // ESC[2J — clear entire screen (resets cursor to home)
        print_str("\x1b[2J");
        test_eq!(ROW.load(Ordering::SeqCst), 0);
        test_eq!(COL.load(Ordering::SeqCst), 0);

        // Restore
        ROW.store(saved_row, Ordering::SeqCst);
        COL.store(saved_col, Ordering::SeqCst);
    });

    test_case!("ansi_256_color", {
        let saved_fg = ANSI_FG.load(Ordering::Relaxed);
        let saved_bg = ANSI_BG.load(Ordering::Relaxed);
        let saved_bold = ANSI_BOLD.load(Ordering::Relaxed);

        // ESC[38;5;82m — 256-color fg (green: index 82 = cube 1,3,4)
        print_str("\x1b[38;5;82m");
        let fg = ANSI_FG.load(Ordering::Relaxed);
        test_eq!(dec_mode(fg), CM_256);
        test_eq!(dec_val(fg), 82);
        // xterm 82 = cube 1,3,4 → RGB(0, 215, 0) in case value is right
        test_eq!(xterm_256_to_rgb(82), 0x005FFF00);

        // ESC[48;5;196m — 256-color bg (bright red)
        print_str("\x1b[48;5;196m");
        let bg = ANSI_BG.load(Ordering::Relaxed);
        test_eq!(dec_mode(bg), CM_256);
        test_eq!(dec_val(bg), 196);
        test_eq!(xterm_256_to_rgb(196), 0xFF0000);

        // Reset clears extended colors
        print_str("\x1b[0m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), enc_color(CM_ANSI, 7));
        test_eq!(ANSI_BG.load(Ordering::Relaxed), enc_color(CM_ANSI, 0));

        ANSI_FG.store(saved_fg, Ordering::Relaxed);
        ANSI_BG.store(saved_bg, Ordering::Relaxed);
        ANSI_BOLD.store(saved_bold, Ordering::Relaxed);
    });

    test_case!("ansi_truecolor", {
        let saved_fg = ANSI_FG.load(Ordering::Relaxed);
        let saved_bg = ANSI_BG.load(Ordering::Relaxed);
        let saved_bold = ANSI_BOLD.load(Ordering::Relaxed);

        // ESC[38;2;100;200;50m — truecolor fg
        print_str("\x1b[38;2;100;200;50m");
        let fg = ANSI_FG.load(Ordering::Relaxed);
        test_eq!(dec_mode(fg), CM_TRUECOLOR);
        test_eq!(dec_val(fg), 0x64C832);

        // ESC[48;2;10;20;30m — truecolor bg
        print_str("\x1b[48;2;10;20;30m");
        let bg = ANSI_BG.load(Ordering::Relaxed);
        test_eq!(dec_mode(bg), CM_TRUECOLOR);
        test_eq!(dec_val(bg), 0x0A141E);

        // Reset clears truecolor
        print_str("\x1b[0m");
        test_eq!(ANSI_FG.load(Ordering::Relaxed), enc_color(CM_ANSI, 7));
        test_eq!(ANSI_BG.load(Ordering::Relaxed), enc_color(CM_ANSI, 0));

        ANSI_FG.store(saved_fg, Ordering::Relaxed);
        ANSI_BG.store(saved_bg, Ordering::Relaxed);
        ANSI_BOLD.store(saved_bold, Ordering::Relaxed);
    });
}
