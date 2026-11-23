//! Behaviour of [`Vt`] against the real libghostty-vt.

use std::sync::Arc;
use std::time::Duration;

use artalde_vt::{
    Attrs, Colors, CursorStyle, Event, Frame, Geometry, Grapheme, Key, KeyAction, KeyInput, Mods,
    MouseAction, MouseButton, MouseInput, Options, Rgb, Scroll, SelectPoint, SelectStep,
    StatusKind, StatusState, Underline, Vt,
};

const GEOMETRY: Geometry = Geometry {
    cell_w: 10.0,
    cell_h: 20.0,
    pad_x: 0.0,
    pad_y: 0.0,
};

fn vt(cols: u16, rows: u16) -> Vt {
    Vt::new(&Options {
        cols,
        rows,
        ..Options::default()
    })
    .expect("terminal")
}

fn line(frame: &Frame, y: usize) -> String {
    let mut out = String::new();
    for cell in &frame.lines[y].cells {
        if cell.attrs.contains(Attrs::SPACER) {
            continue;
        }
        if cell.text.is_empty() {
            out.push(' ');
        } else {
            cell.text.push_to(&mut out);
        }
    }
    out.truncate(out.trim_end().len());
    out
}

fn theme() -> Colors {
    let mut ansi = [Rgb::new(0, 0, 0); 16];
    for (i, slot) in ansi.iter_mut().enumerate() {
        *slot = Rgb::new(10 + i as u8, 20 + i as u8, 30 + i as u8);
    }
    Colors {
        foreground: Rgb::new(200, 201, 202),
        background: Rgb::new(1, 2, 3),
        cursor: Some(Rgb::new(9, 8, 7)),
        selection: Some((Rgb::new(50, 60, 70), None)),
        ansi,
    }
}

fn point(col: u16, row: u16, x: f32) -> SelectPoint {
    SelectPoint {
        col,
        row,
        x,
        y: f32::from(row) * GEOMETRY.cell_h + 5.0,
        geometry: GEOMETRY,
        rectangle: false,
    }
}

fn click(action: MouseAction, button: MouseButton, x: f32, y: f32) -> MouseInput {
    MouseInput {
        action,
        button: Some(button),
        mods: Mods::NONE,
        x,
        y,
        geometry: GEOMETRY,
    }
}

#[test]
fn plain_text_shows_in_frame() {
    let mut vt = vt(20, 4);
    vt.write(b"hello\r\nworld");
    let frame = vt.frame();
    assert_eq!((frame.cols, frame.rows), (20, 4));
    assert_eq!(frame.lines.len(), 4);
    assert!(frame.lines.iter().all(|line| line.cells.len() == 20));
    assert_eq!(line(&frame, 0), "hello");
    assert_eq!(line(&frame, 1), "world");
    assert_eq!(line(&frame, 2), "");
    assert_eq!(vt.size(), (20, 4));
    assert_eq!(vt.row_text(1), "world");
    assert!(frame.at_bottom);
}

#[test]
fn sgr_colors_and_attributes() {
    let colors = theme();
    let mut vt = Vt::new(&Options {
        cols: 20,
        rows: 3,
        colors: Some(colors.clone()),
        ..Options::default()
    })
    .unwrap();
    vt.write(
        b"\x1b[31mR\x1b[0m\x1b[1mB\x1b[0m\x1b[4mU\x1b[0m\x1b[7mI\x1b[0m\x1b[38;2;1;2;3;48;5;4mT\x1b[0m\
          \x1b[3;9;4:3;58;2;7;8;9mX\x1b[0mp",
    );
    let frame = vt.frame();
    assert_eq!(frame.foreground, colors.foreground);
    assert_eq!(frame.background, colors.background);
    let cells = &frame.lines[0].cells;

    assert_eq!(cells[0].fg, Some(colors.ansi[1]));
    assert_eq!(cells[0].bg, None);
    assert!(cells[1].attrs.contains(Attrs::BOLD));
    assert_eq!(cells[1].fg, None);
    assert_eq!(cells[2].underline, Underline::Single);
    assert!(!cells[2].attrs.contains(Attrs::BOLD));
    // Inverse arrives resolved.
    assert_eq!(cells[3].fg, Some(colors.background));
    assert_eq!(cells[3].bg, Some(colors.foreground));
    assert_eq!(cells[4].fg, Some(Rgb::new(1, 2, 3)));
    assert_eq!(cells[4].bg, Some(colors.ansi[4]));
    assert!(cells[5].attrs.contains(Attrs::ITALIC | Attrs::STRIKE));
    assert_eq!(cells[5].underline, Underline::Curly);
    assert_eq!(cells[5].underline_color, Some(Rgb::new(7, 8, 9)));
    assert!(cells[6].attrs.is_empty());
    assert_eq!(cells[6].fg, None);
    assert_eq!(frame.cursor.unwrap().color, colors.cursor);
}

#[test]
fn set_colors_restyles_every_row() {
    let mut vt = vt(10, 2);
    vt.write(b"\x1b[32mg");
    let before = vt.frame();
    assert!(!vt.changed());
    let colors = theme();
    vt.set_colors(&colors);
    assert!(vt.changed());
    let after = vt.frame();
    assert_eq!(after.background, colors.background);
    assert_eq!(after.lines[0].cells[0].fg, Some(colors.ansi[2]));
    assert_ne!(before.lines[0].cells[0].fg, after.lines[0].cells[0].fg);
}

#[test]
fn wide_characters_and_emoji_clusters() {
    let mut vt = vt(20, 2);
    vt.write("你好a".as_bytes());
    vt.write("\r\n\u{2764}\u{fe0f}b".as_bytes());
    let frame = vt.frame();
    let cells = &frame.lines[0].cells;
    assert_eq!(cells[0].text.to_string(), "你");
    assert!(cells[0].attrs.contains(Attrs::WIDE));
    assert!(cells[1].attrs.contains(Attrs::SPACER));
    assert!(cells[1].text.is_empty());
    assert_eq!(cells[2].text.to_string(), "好");
    assert_eq!(cells[4].text.to_string(), "a");
    assert!(!cells[4].attrs.contains(Attrs::WIDE));

    // U+2764 is narrow on its own; VS16 makes it an emoji, two cells wide.
    let cells = &frame.lines[1].cells;
    assert_eq!(cells[0].text.to_string(), "\u{2764}\u{fe0f}");
    assert_eq!(cells[0].text.cluster(), Some("\u{2764}\u{fe0f}"));
    assert!(cells[0].attrs.contains(Attrs::WIDE));
    assert!(cells[1].attrs.contains(Attrs::SPACER));
    assert_eq!(cells[2].text.to_string(), "b");
    assert_eq!(frame.cursor.unwrap().x, 3);
}

#[test]
fn wrap_flag_marks_soft_wrapped_rows() {
    let mut vt = vt(10, 4);
    vt.write(b"0123456789abc\r\nshort");
    let frame = vt.frame();
    assert!(frame.lines[0].wrapped);
    assert!(!frame.lines[1].wrapped);
    assert!(!frame.lines[2].wrapped);
    assert_eq!(line(&frame, 1), "abc");
}

#[test]
fn cursor_position_and_style() {
    let mut vt = vt(20, 5);
    vt.write(b"ab\r\ncde");
    let cursor = vt.frame().cursor.unwrap();
    assert_eq!((cursor.x, cursor.y), (3, 1));
    assert_eq!(cursor.style, CursorStyle::Block);

    vt.write(b"\x1b[5 q");
    let cursor = vt.frame().cursor.unwrap();
    assert_eq!(cursor.style, CursorStyle::Bar);
    assert!(cursor.blinking);

    vt.write(b"\x1b[4 q");
    let cursor = vt.frame().cursor.unwrap();
    assert_eq!(cursor.style, CursorStyle::Underline);
    assert!(!cursor.blinking);

    vt.write(b"\x1b[2 q\x1b[4;7H");
    let cursor = vt.frame().cursor.unwrap();
    assert_eq!(cursor.style, CursorStyle::Block);
    assert_eq!((cursor.x, cursor.y), (6, 3));

    vt.write(b"\x1b[?25l");
    assert!(vt.changed());
    assert_eq!(vt.frame().cursor, None);
    vt.write(b"\x1b[?25h");
    assert!(vt.frame().cursor.is_some());
}

#[test]
fn resize_reflows_wrapped_lines() {
    let mut vt = vt(10, 4);
    vt.write(b"0123456789abcdef");
    let frame = vt.frame();
    assert!(frame.lines[0].wrapped);
    assert_eq!(line(&frame, 1), "abcdef");

    vt.resize(20, 4, 8, 16);
    assert!(vt.changed());
    let frame = vt.frame();
    assert_eq!((frame.cols, frame.rows), (20, 4));
    assert_eq!(frame.lines[0].cells.len(), 20);
    assert_eq!(line(&frame, 0), "0123456789abcdef");
    assert!(!frame.lines[0].wrapped);
    assert_eq!(line(&frame, 1), "");

    vt.resize(4, 6, 8, 16);
    let frame = vt.frame();
    assert_eq!(frame.lines.len(), 6);
    assert_eq!(line(&frame, 0), "0123");
    assert_eq!(line(&frame, 3), "cdef");
    assert!(frame.lines[2].wrapped);
    assert_eq!(vt.size(), (4, 6));
}

#[test]
fn scrollback_scrolling_and_scrollbar() {
    let mut vt = vt(20, 5);
    for i in 0..20 {
        vt.write(format!("line {i}\r\n").as_bytes());
    }
    let frame = vt.frame();
    assert!(frame.at_bottom);
    assert_eq!(frame.scrollbar.len, 5);
    assert_eq!(frame.scrollbar.total, 21);
    assert_eq!(frame.scrollbar.offset, 16);
    assert_eq!(line(&frame, 0), "line 16");
    assert!(!vt.changed());

    vt.scroll(Scroll::Lines(-3));
    assert!(vt.changed());
    let frame = vt.frame();
    assert!(!frame.at_bottom);
    assert_eq!(frame.scrollbar.offset, 13);
    assert_eq!(line(&frame, 0), "line 13");
    assert_eq!(frame.cursor, None);

    vt.scroll(Scroll::Pages(-1));
    assert_eq!(vt.frame().scrollbar.offset, 8);

    vt.scroll(Scroll::Top);
    let frame = vt.frame();
    assert_eq!(frame.scrollbar.offset, 0);
    assert_eq!(line(&frame, 0), "line 0");
    assert_eq!(vt.row_text(0), "line 0");

    vt.scroll(Scroll::ToRow(4));
    assert_eq!(line(&vt.frame(), 0), "line 4");

    vt.scroll(Scroll::Bottom);
    let frame = vt.frame();
    assert!(frame.at_bottom);
    assert_eq!(line(&frame, 3), "line 19");
    assert!(frame.cursor.is_some());
}

#[test]
fn unchanged_rows_keep_their_arc() {
    let mut vt = vt(20, 4);
    vt.write(b"one\r\ntwo\r\nthree");
    let first = vt.frame();
    let again = vt.frame();
    for y in 0..4 {
        assert!(Arc::ptr_eq(&first.lines[y], &again.lines[y]), "row {y}");
    }

    vt.write(b"\x1b[2;1Hchanged");
    let second = vt.frame();
    assert!(Arc::ptr_eq(&first.lines[0], &second.lines[0]));
    assert!(!Arc::ptr_eq(&first.lines[1], &second.lines[1]));
    assert!(Arc::ptr_eq(&first.lines[2], &second.lines[2]));
    assert!(Arc::ptr_eq(&first.lines[3], &second.lines[3]));
    assert_eq!(line(&second, 1), "changed");
    assert_eq!(line(&first, 1), "two");

    // A write that leaves a row as it was does not replace it either.
    vt.write(b"\x1b[1;1Hone");
    let third = vt.frame();
    assert!(Arc::ptr_eq(&second.lines[0], &third.lines[0]));
}

#[test]
fn changed_follows_the_screen() {
    let mut vt = vt(20, 4);
    assert!(vt.changed(), "nothing drawn yet");
    let _ = vt.frame();
    assert!(!vt.changed());
    assert!(!vt.changed());

    vt.write(b"x");
    assert!(vt.changed());
    assert!(vt.changed(), "stays true until a frame is taken");
    let _ = vt.frame();
    assert!(!vt.changed());

    vt.write(b"\x1b[3;3H");
    assert!(vt.changed(), "cursor moved");
    let _ = vt.frame();
    assert!(!vt.changed());

    vt.write(b"\x1b]0;title only\x07");
    assert!(!vt.changed(), "a title is not part of the frame");

    vt.select_all();
    assert!(vt.changed());
    let _ = vt.frame();
    vt.clear_selection();
    assert!(vt.changed());
    let _ = vt.frame();
    assert!(!vt.changed());
}

#[test]
fn queries_are_answered() {
    let mut vt = vt(20, 4);
    assert!(vt.take_replies().is_empty());
    vt.write(b"\x1b[c");
    assert_eq!(vt.take_replies(), b"\x1b[?62;22c");
    vt.write(b"\x1b[>c");
    let secondary = vt.take_replies();
    assert!(secondary.starts_with(b"\x1b[>1;10;0c"), "{secondary:?}");
    vt.write(b"\x1b[5n");
    assert_eq!(vt.take_replies(), b"\x1b[0n");
    vt.write(b"ab\r\ncd\x1b[6n");
    assert_eq!(vt.take_replies(), b"\x1b[2;3R");
    assert!(vt.take_replies().is_empty());

    // Size queries are answered once the cell size is known.
    vt.write(b"\x1b[18t");
    vt.resize(20, 4, 9, 18);
    let _ = vt.take_replies();
    vt.write(b"\x1b[18t");
    assert_eq!(vt.take_replies(), b"\x1b[8;4;20t");
    vt.write(b"\x1b[14t");
    assert_eq!(vt.take_replies(), b"\x1b[4;72;180t");
}

#[test]
fn events_are_reported() {
    let mut vt = vt(20, 4);
    assert!(vt.take_events().is_empty());
    assert_eq!(vt.title(), None);
    assert_eq!(vt.pwd(), None);

    vt.write(b"\x1b]2;first\x07\x1b]2;hello\x07");
    assert_eq!(vt.take_events(), [Event::Title("hello".to_owned())]);
    assert_eq!(vt.title().as_deref(), Some("hello"));

    vt.write(b"\x07\x07\x07");
    assert_eq!(vt.take_events(), [Event::Bell]);

    vt.write(b"\x1b]7;file://host/tmp/dir\x07");
    assert_eq!(
        vt.take_events(),
        [Event::Pwd("file://host/tmp/dir".to_owned())]
    );
    assert_eq!(vt.pwd().as_deref(), Some("file://host/tmp/dir"));

    vt.write(b"\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(vt.take_events(), [Event::Clipboard("hello".to_owned())]);
    // Reading the clipboard is never granted.
    vt.write(b"\x1b]52;c;?\x07");
    assert!(vt.take_events().is_empty());

    vt.write(b"\x1b]7501;state=blocked:kind=permission:app=cargo:id=build:msg=UGxhbg\x1b\\");
    vt.write(b"\x1b]7501;state=working:progress=40\x1b\\");
    let events = vt.take_events();
    assert_eq!(events.len(), 2, "{events:?}");
    let Event::Status(status) = &events[0] else {
        panic!("{events:?}");
    };
    assert_eq!(status.state, StatusState::Blocked);
    assert_eq!(status.kind, StatusKind::Permission);
    assert_eq!(status.app, "cargo");
    assert_eq!(status.id, "build");
    assert_eq!(status.message, "Plan");
    assert_eq!(status.progress, None);
    let Event::Status(status) = &events[1] else {
        panic!("{events:?}");
    };
    assert_eq!(status.state, StatusState::Working);
    assert_eq!(status.kind, StatusKind::None);
    assert_eq!(status.progress, Some(40));
    assert!(vt.take_events().is_empty());
}

#[test]
fn oversized_clipboard_writes_are_dropped() {
    let mut vt = vt(20, 4);
    let mut sequence = b"\x1b]52;c;".to_vec();
    // 1.5 MiB once decoded.
    sequence.extend(std::iter::repeat_n(b"QUFB".as_slice(), 512 * 1024).flatten());
    sequence.push(0x07);
    vt.write(&sequence);
    assert!(vt.take_events().is_empty());
    vt.write(b"\x1b]52;c;b2s=\x07");
    assert_eq!(vt.take_events(), [Event::Clipboard("ok".to_owned())]);
}

#[test]
fn keys_encode_for_the_current_modes() {
    let mut vt = vt(20, 4);
    assert_eq!(vt.key(&KeyInput::press(Key::Enter)), b"\r");
    assert_eq!(vt.key(&KeyInput::press(Key::Tab)), b"\t");
    assert_eq!(vt.key(&KeyInput::press(Key::Escape)), b"\x1b");
    assert_eq!(vt.key(&KeyInput::press(Key::Backspace)), b"\x7f");
    assert_eq!(vt.key(&KeyInput::press(Key::F5)), b"\x1b[15~");

    let plain = KeyInput {
        text: Some("a"),
        unshifted: Some('a'),
        ..KeyInput::press(Key::A)
    };
    assert_eq!(vt.key(&plain), b"a");

    let ctrl_c = KeyInput {
        mods: Mods::CTRL,
        unshifted: Some('c'),
        ..KeyInput::press(Key::C)
    };
    assert_eq!(vt.key(&ctrl_c), b"\x03");

    assert_eq!(vt.key(&KeyInput::press(Key::ArrowUp)), b"\x1b[A");
    vt.write(b"\x1b[?1h");
    assert!(vt.mode(1));
    assert_eq!(vt.key(&KeyInput::press(Key::ArrowUp)), b"\x1bOA");
    assert_eq!(vt.key(&KeyInput::press(Key::ArrowLeft)), b"\x1bOD");
    vt.write(b"\x1b[?1l");
    assert_eq!(vt.key(&KeyInput::press(Key::ArrowDown)), b"\x1b[B");
    let shift_right = KeyInput {
        mods: Mods::SHIFT,
        ..KeyInput::press(Key::ArrowRight)
    };
    assert_eq!(vt.key(&shift_right), b"\x1b[1;2C");

    let bang = KeyInput {
        mods: Mods::SHIFT,
        consumed: Mods::SHIFT,
        text: Some("!"),
        unshifted: Some('1'),
        ..KeyInput::press(Key::Digit1)
    };
    assert_eq!(vt.key(&bang), b"!");

    let alt_a = KeyInput {
        mods: Mods::ALT,
        text: Some("a"),
        unshifted: Some('a'),
        option_as_alt: true,
        ..KeyInput::press(Key::A)
    };
    assert_eq!(vt.key(&alt_a), b"\x1ba");

    let release = KeyInput {
        action: KeyAction::Release,
        ..plain
    };
    assert!(vt.key(&release).is_empty(), "legacy mode has no releases");
    assert!(vt.key(&KeyInput::press(Key::Unidentified)).is_empty());
    let composed = KeyInput {
        text: Some("é"),
        ..KeyInput::press(Key::Unidentified)
    };
    assert_eq!(vt.key(&composed), "é".as_bytes());
}

#[test]
fn keys_encode_with_the_kitty_protocol() {
    let mut vt = vt(20, 4);
    assert_eq!(vt.kitty_keyboard_flags(), 0);
    // Disambiguate + report event types.
    vt.write(b"\x1b[>3u");
    assert_eq!(vt.kitty_keyboard_flags(), 3);
    assert_eq!(vt.key(&KeyInput::press(Key::Escape)), b"\x1b[27u");
    let ctrl_c = KeyInput {
        mods: Mods::CTRL,
        unshifted: Some('c'),
        ..KeyInput::press(Key::C)
    };
    assert_eq!(vt.key(&ctrl_c), b"\x1b[99;5u");
    let release = KeyInput {
        action: KeyAction::Release,
        ..ctrl_c
    };
    assert_eq!(vt.key(&release), b"\x1b[99;5:3u");
    let plain = KeyInput {
        text: Some("a"),
        unshifted: Some('a'),
        ..KeyInput::press(Key::A)
    };
    assert_eq!(vt.key(&plain), b"a");
    vt.write(b"\x1b[<u");
    assert_eq!(vt.kitty_keyboard_flags(), 0);
    assert_eq!(vt.key(&KeyInput::press(Key::Escape)), b"\x1b");
}

#[test]
fn mouse_reports_only_when_tracked() {
    let mut vt = vt(20, 5);
    let press = click(MouseAction::Press, MouseButton::Left, 25.0, 30.0);
    let release = click(MouseAction::Release, MouseButton::Left, 25.0, 30.0);
    let wheel = click(MouseAction::Press, MouseButton::WheelUp, 25.0, 30.0);
    assert!(!vt.mouse_tracking());
    assert!(vt.mouse(&press).is_empty());
    assert!(vt.mouse(&release).is_empty());
    assert!(vt.mouse(&wheel).is_empty());

    vt.write(b"\x1b[?1000h\x1b[?1006h");
    assert!(vt.mouse_tracking());
    assert_eq!(vt.mouse(&press), b"\x1b[<0;3;2M");
    assert_eq!(vt.mouse(&release), b"\x1b[<0;3;2m");
    assert_eq!(vt.mouse(&wheel), b"\x1b[<64;3;2M");
    let wheel_down = click(MouseAction::Press, MouseButton::WheelDown, 199.0, 99.0);
    assert_eq!(vt.mouse(&wheel_down), b"\x1b[<65;20;5M");
    let right = MouseInput {
        mods: Mods::CTRL,
        ..click(MouseAction::Press, MouseButton::Right, 0.0, 0.0)
    };
    assert_eq!(vt.mouse(&right), b"\x1b[<18;1;1M");
    let _ = vt.mouse(&click(MouseAction::Release, MouseButton::Right, 0.0, 0.0));

    // Normal tracking does not report motion.
    let motion = click(MouseAction::Motion, MouseButton::Left, 55.0, 30.0);
    assert!(vt.mouse(&motion).is_empty());

    // Button-event tracking reports a drag once per cell.
    vt.write(b"\x1b[?1002h");
    assert_eq!(vt.mouse(&press), b"\x1b[<0;3;2M");
    assert_eq!(vt.mouse(&motion), b"\x1b[<32;6;2M");
    let same_cell = click(MouseAction::Motion, MouseButton::Left, 58.0, 31.0);
    assert!(vt.mouse(&same_cell).is_empty());
    let next_cell = click(MouseAction::Motion, MouseButton::Left, 61.0, 31.0);
    assert_eq!(vt.mouse(&next_cell), b"\x1b[<32;7;2M");

    vt.write(b"\x1b[?1002l\x1b[?1000l");
    assert!(!vt.mouse_tracking());
    assert!(vt.mouse(&press).is_empty());
}

#[test]
fn mouse_cells_do_not_drift_with_fractional_cell_sizes() {
    let mut vt = vt(200, 5);
    vt.write(b"\x1b[?1000h\x1b[?1006h");
    let geometry = Geometry {
        cell_w: 8.4,
        cell_h: 17.5,
        pad_x: 6.0,
        pad_y: 4.0,
    };
    for col in [0u16, 1, 57, 150, 199] {
        // The middle of the cell.
        let input = MouseInput {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            mods: Mods::NONE,
            x: geometry.pad_x + (f32::from(col) + 0.5) * geometry.cell_w,
            y: geometry.pad_y + 3.5 * geometry.cell_h,
            geometry,
        };
        let expected = format!("\x1b[<0;{};4M", col + 1);
        assert_eq!(vt.mouse(&input), expected.as_bytes(), "column {col}");
    }
}

#[test]
fn paste_is_bracketed_on_request() {
    let mut vt = vt(20, 4);
    assert!(!vt.bracketed_paste());
    assert_eq!(vt.paste("a\nb"), b"a\rb");
    assert!(vt.paste("").is_empty());

    vt.write(b"\x1b[?2004h");
    assert!(vt.bracketed_paste());
    assert_eq!(vt.paste("a\nb"), b"\x1b[200~a\nb\x1b[201~");
    // The end marker cannot be smuggled in.
    let hostile = vt.paste("x\x1b[201~rm -rf");
    assert!(hostile.starts_with(b"\x1b[200~"));
    assert!(hostile.ends_with(b"\x1b[201~"));
    let inner = &hostile[6..hostile.len() - 6];
    assert!(!inner.windows(6).any(|window| window == b"\x1b[201~"));

    assert!(Vt::paste_is_safe("hello world"));
    assert!(!Vt::paste_is_safe("ls\nrm -rf /"));
    assert!(!Vt::paste_is_safe("x\x1b[201~y"));
}

#[test]
fn focus_reports_on_request() {
    let mut vt = vt(20, 4);
    assert!(vt.focus(true).is_empty());
    vt.write(b"\x1b[?1004h");
    assert_eq!(vt.focus(true), b"\x1b[I");
    assert_eq!(vt.focus(false), b"\x1b[O");
    vt.write(b"\x1b[?1004l");
    assert!(vt.focus(false).is_empty());
}

#[test]
fn modes_are_visible() {
    let mut vt = vt(20, 4);
    assert!(!vt.alt_screen());
    assert!(vt.mode(2027), "grapheme clustering is on by default");
    vt.write(b"\x1b[?1049h");
    assert!(vt.alt_screen());
    assert!(vt.mode(1049));
    vt.write(b"\x1b[?1049l");
    assert!(!vt.alt_screen());
}

#[test]
fn drag_selects_text() {
    let mut vt = vt(20, 4);
    vt.write(b"hello world\r\nsecond line");
    let _ = vt.frame();
    assert!(!vt.has_selection());
    assert_eq!(vt.selection_text(), None);

    vt.select(SelectStep::Press, point(0, 0, 1.0));
    assert!(!vt.has_selection(), "a press alone selects nothing");
    vt.select(SelectStep::Drag, point(4, 0, 48.0));
    vt.select(SelectStep::Release, point(4, 0, 48.0));
    assert!(vt.has_selection());
    assert_eq!(vt.selection_text().as_deref(), Some("hello"));

    assert!(vt.changed());
    let frame = vt.frame();
    let cells = &frame.lines[0].cells;
    assert!(cells[..5].iter().all(|c| c.attrs.contains(Attrs::SELECTED)));
    assert!(!cells[5].attrs.contains(Attrs::SELECTED));
    assert!(
        frame.lines[1]
            .cells
            .iter()
            .all(|c| !c.attrs.contains(Attrs::SELECTED))
    );

    // Across rows.
    vt.select(SelectStep::Press, point(6, 0, 61.0));
    vt.select(SelectStep::Drag, point(5, 1, 59.0));
    vt.select(SelectStep::Release, point(5, 1, 59.0));
    assert_eq!(vt.selection_text().as_deref(), Some("world\nsecond"));

    vt.clear_selection();
    assert!(!vt.has_selection());
    let frame = vt.frame();
    assert!(
        frame.lines[0]
            .cells
            .iter()
            .all(|c| !c.attrs.contains(Attrs::SELECTED))
    );

    vt.select_all();
    assert_eq!(
        vt.selection_text().as_deref(),
        Some("hello world\nsecond line")
    );
}

#[test]
fn selection_uses_theme_colors() {
    let colors = theme();
    let mut vt = Vt::new(&Options {
        cols: 20,
        rows: 2,
        colors: Some(colors.clone()),
        ..Options::default()
    })
    .unwrap();
    vt.write(b"\x1b[31mabc");
    vt.select_all();
    let frame = vt.frame();
    let cell = &frame.lines[0].cells[0];
    assert_eq!(cell.bg, Some(Rgb::new(50, 60, 70)));
    assert_eq!(cell.fg, Some(colors.ansi[1]));
}

#[test]
fn double_click_selects_a_word_and_a_click_clears() {
    let mut vt = vt(20, 4);
    vt.write(b"hello world");
    let at = point(7, 0, 75.0);
    vt.select(SelectStep::Press, at);
    vt.select(SelectStep::Release, at);
    assert!(!vt.has_selection());
    vt.select(SelectStep::Press, at);
    vt.select(SelectStep::Release, at);
    assert_eq!(vt.selection_text().as_deref(), Some("world"));
    // Third press: the line.
    vt.select(SelectStep::Press, at);
    vt.select(SelectStep::Release, at);
    assert_eq!(vt.selection_text().as_deref(), Some("hello world"));

    std::thread::sleep(Duration::from_millis(600));
    vt.select(SelectStep::Press, point(1, 0, 15.0));
    vt.select(SelectStep::Release, point(1, 0, 15.0));
    assert!(!vt.has_selection(), "a plain click drops the selection");
}

#[test]
fn hyperlinks_are_found() {
    let mut vt = vt(30, 3);
    vt.write(b"\x1b]8;;https://example.com/a\x1b\\link\x1b]8;;\x1b\\ plain");
    assert_eq!(vt.link_at(0, 0).as_deref(), Some("https://example.com/a"));
    assert_eq!(vt.link_at(3, 0).as_deref(), Some("https://example.com/a"));
    assert_eq!(vt.link_at(4, 0), None);
    assert_eq!(vt.link_at(7, 0), None);
    assert_eq!(vt.link_at(0, 2), None);
    assert_eq!(vt.link_at(500, 500), None);
    let frame = vt.frame();
    let cells = &frame.lines[0].cells;
    assert!(cells[..4].iter().all(|c| c.attrs.contains(Attrs::LINK)));
    assert!(!cells[4].attrs.contains(Attrs::LINK));
}

#[test]
fn text_includes_requested_history() {
    let mut vt = vt(20, 3);
    assert_eq!(vt.text(0), "");
    for i in 0..6 {
        vt.write(format!("row {i}  \r\n").as_bytes());
    }
    vt.write(b"last");
    assert_eq!(vt.text(0), "row 4\nrow 5\nlast");
    assert_eq!(vt.text(2), "row 2\nrow 3\nrow 4\nrow 5\nlast");
    assert_eq!(
        vt.text(1000),
        "row 0\nrow 1\nrow 2\nrow 3\nrow 4\nrow 5\nlast"
    );
    // Scrolling the viewport does not change what `text` returns.
    vt.scroll(Scroll::Top);
    assert_eq!(vt.text(0), "row 4\nrow 5\nlast");
}

fn assert_same_screen(original: &mut Vt, copy: &mut Vt, history: usize) {
    assert_eq!(copy.text(history), original.text(history));
    assert_eq!(copy.alt_screen(), original.alt_screen());
    let a = original.frame();
    let b = copy.frame();
    assert_eq!(b.cursor, a.cursor);
    // A repaint writes spaces where the original never wrote anything; the
    // two draw the same.
    let normalized = |frame: &Frame, y: usize| {
        let mut line = (*frame.lines[y]).clone();
        for cell in &mut line.cells {
            if cell.text.first() == Some(' ') {
                cell.text = Grapheme::EMPTY;
            }
        }
        line
    };
    for y in 0..a.lines.len() {
        assert_eq!(normalized(&b, y), normalized(&a, y), "row {y}");
    }
}

#[test]
fn repaint_reproduces_the_primary_screen() {
    let mut original = vt(30, 6);
    original.write(
        b"\x1b]2;my title\x07plain \x1b[1;31mbold red\x1b[0m\r\n\xe4\xbd\xa0\xe5\xa5\xbd wide\r\n\
          \x1b[4munder\x1b[0m \x1b[7minv\x1b[0m\x1b[5;10Hhere\x1b[3 q\x1b[?2004h\x1b[?1h",
    );
    let bytes = original.repaint(0);
    let mut copy = vt(30, 6);
    // A viewer that already showed something else.
    copy.write(b"stale junk\r\n\x1b[?1049hmore junk\x1b[?1000h");
    copy.write(&bytes);
    assert_same_screen(&mut original, &mut copy, 0);
    assert_eq!(copy.title().as_deref(), Some("my title"));
    assert!(copy.bracketed_paste());
    assert!(copy.mode(1));
    assert!(!copy.mouse_tracking());
    let cursor = copy.frame().cursor.unwrap();
    assert_eq!((cursor.x, cursor.y), (13, 4));
    assert_eq!(cursor.style, CursorStyle::Underline);
}

#[test]
fn repaint_reproduces_the_alternate_screen_and_what_is_under_it() {
    let mut original = vt(30, 6);
    original.write(b"shell prompt\r\n$ vim\r\n");
    original.write(b"\x1b[?1049h\x1b[2J\x1b[Hfull screen app\x1b[3;5H\x1b[32mgreen\x1b[0m\x1b[?1000h\x1b[?1006h");
    let bytes = original.repaint(100);
    let mut copy = vt(30, 6);
    copy.write(&bytes);
    assert!(copy.alt_screen());
    assert!(copy.mouse_tracking());
    assert_same_screen(&mut original, &mut copy, 0);

    // Leaving the alternate screen reveals the same primary screen in both.
    original.write(b"\x1b[?1049l");
    copy.write(b"\x1b[?1049l");
    assert_same_screen(&mut original, &mut copy, 0);
    assert_eq!(copy.text(0), "shell prompt\n$ vim");
}

#[test]
fn repaint_reproduces_scrollback() {
    let mut original = vt(20, 5);
    for i in 0..40 {
        original.write(format!("\x1b[3{}mentry {i}\x1b[0m\r\n", i % 8).as_bytes());
    }
    original.write(b"a line long enough to wrap around\r\nprompt> ");

    let mut copy = vt(20, 5);
    copy.write(&original.repaint(1000));
    assert_same_screen(&mut original, &mut copy, 1000);
    assert_eq!(copy.frame().scrollbar, original.frame().scrollbar);
    copy.scroll(Scroll::Top);
    original.scroll(Scroll::Top);
    assert_same_screen(&mut original, &mut copy, 1000);

    // A bounded history.
    let mut copy = vt(20, 5);
    copy.write(&original.repaint(7));
    assert_eq!(copy.text(1000), original.text(7));

    // None at all.
    let mut copy = vt(20, 5);
    copy.write(&original.repaint(0));
    assert_eq!(copy.text(1000), original.text(0));
}

#[test]
fn search_finds_matches() {
    let mut vt = vt(30, 5);
    for i in 0..30 {
        if i % 10 == 3 {
            vt.write(format!("{i}: a Needle here\r\n").as_bytes());
        } else {
            vt.write(format!("{i}: hay\r\n").as_bytes());
        }
    }
    vt.write(b"needle at the bottom");
    let _ = vt.frame();

    assert!(vt.search_matches().is_empty());
    vt.set_needle(Some("needle"));
    let mut status = vt.search_tick();
    for _ in 0..1000 {
        if status.complete {
            break;
        }
        status = vt.search_tick();
    }
    assert!(status.complete);
    assert_eq!(status.total, 4);

    let matches = vt.search_matches();
    assert_eq!(matches.len(), 1, "{matches:?}");
    assert_eq!(
        (matches[0].row, matches[0].col_start, matches[0].col_end),
        (4, 0, 6)
    );

    let status = vt.search_next();
    assert_eq!(status.selected, Some(0));
    assert!(vt.search_matches()[0].selected);
    let status = vt.search_next();
    assert_eq!(status.selected, Some(1));
    assert!(vt.changed());
    let frame = vt.frame();
    assert!(!frame.at_bottom, "the older match was scrolled into view");
    let matches = vt.search_matches();
    let selected: Vec<_> = matches.iter().filter(|m| m.selected).collect();
    assert_eq!(selected.len(), 1, "{matches:?}");
    assert_eq!(
        line(&frame, usize::from(selected[0].row)),
        "23: a Needle here"
    );
    assert_eq!((selected[0].col_start, selected[0].col_end), (6, 12));
    let status = vt.search_prev();
    assert_eq!(status.selected, Some(0));

    // New output is picked up by the next tick.
    vt.scroll(Scroll::Bottom);
    vt.write(b"\r\nanother needle");
    let mut status = vt.search_tick();
    for _ in 0..1000 {
        if status.complete && status.total == 5 {
            break;
        }
        status = vt.search_tick();
    }
    assert_eq!(status.total, 5);

    vt.set_needle(None);
    assert!(vt.search_matches().is_empty());
    let status = vt.search_tick();
    assert_eq!((status.total, status.selected), (0, None));
    assert_eq!(vt.search_next().total, 0);
}

#[test]
fn synchronized_output_holds_the_frame() {
    let mut vt = vt(20, 3);
    vt.write(b"before");
    let shown = vt.frame();
    assert!(!vt.render_held());

    vt.write(b"\x1b[?2026h");
    assert!(vt.render_held());
    vt.write(b"\x1b[2J\x1b[Hhalf");
    assert!(!vt.changed(), "nothing to draw while held");
    let held = vt.frame();
    assert_eq!(line(&held, 0), "before");
    assert_eq!(held.cursor, shown.cursor);
    assert!(Arc::ptr_eq(&held.lines[0], &shown.lines[0]));
    // The terminal itself is up to date.
    assert_eq!(vt.text(0), "half");

    vt.write(b" done\x1b[?2026l");
    assert!(!vt.render_held());
    assert!(vt.changed());
    assert_eq!(line(&vt.frame(), 0), "half done");
}

#[test]
fn a_hold_captures_what_preceded_it() {
    let mut vt = vt(20, 3);
    let _ = vt.frame();
    // Written but never drawn before the hold begins.
    vt.write(b"first\x1b[?2026h\x1b[2J\x1b[Hsecond");
    assert!(vt.changed());
    assert_eq!(line(&vt.frame(), 0), "first");
    assert!(!vt.changed());
}

#[test]
fn an_overdue_hold_is_broken() {
    let mut vt = vt(20, 3);
    vt.set_hold_timeout(Duration::from_millis(40));
    vt.write(b"before");
    let _ = vt.frame();
    vt.write(b"\x1b[?2026h\x1b[2J\x1b[Hstuck");
    assert!(!vt.changed());
    assert_eq!(line(&vt.frame(), 0), "before");

    std::thread::sleep(Duration::from_millis(80));
    assert!(vt.changed());
    assert!(!vt.render_held());
    assert!(!vt.mode(2026));
    assert_eq!(line(&vt.frame(), 0), "stuck");
}

#[test]
fn scrolling_shows_through_a_hold() {
    let mut vt = vt(20, 3);
    for i in 0..10 {
        vt.write(format!("line {i}\r\n").as_bytes());
    }
    let _ = vt.frame();
    vt.write(b"\x1b[?2026h");
    vt.scroll(Scroll::Top);
    assert!(vt.changed());
    assert_eq!(line(&vt.frame(), 0), "line 0");
}

#[test]
fn kitty_graphics_are_ignored() {
    let mut vt = vt(20, 4);
    vt.resize(20, 4, 10, 20);
    vt.write(b"a\x1b_Gf=24,s=1,v=1,a=T,i=1;AAAA\x1b\\b");
    vt.write(b"\x1b_Ga=q,i=2,s=1,v=1;AAAA\x1b\\");
    vt.write(b"\x1bPq#0;2;0;0;0#0~~@@vv@@~~@@~~$\x1b\\c");
    let frame = vt.frame();
    assert!(line(&frame, 0).starts_with("ab"), "{}", line(&frame, 0));
    let _ = vt.take_replies();
}

/// xorshift64*: a fixed stream, the same on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

#[test]
fn random_bytes_do_not_break_anything() {
    const FRAGMENTS: [&[u8]; 24] = [
        b"\x1b[",
        b"\x1b]",
        b"\x1b[?",
        b"\x1bP",
        b"\x1b_G",
        b"\x1b\\",
        b"\x07",
        b"\x1b[?1049h",
        b"\x1b[?1049l",
        b"\x1b[?2026h",
        b"\x1b[?2026l",
        b"\x1b[2J",
        b"\x1b[H",
        b"\r\n",
        b";",
        b"m",
        b"h",
        b"\x1b[38;2;",
        b"\x1b]8;;http://x\x1b\\",
        b"\x1b]52;c;",
        b"\xf0\x9f\x91\x8d",
        b"\xe4\xbd\xa0",
        b"\x1b[6n",
        b"\x1b[c",
    ];
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    let mut vt = vt(80, 24);
    vt.set_needle(Some("a"));
    let mut written = 0usize;
    let mut chunk = Vec::new();
    while written < 400 * 1024 {
        chunk.clear();
        let len = 1 + random.below(600) as usize;
        while chunk.len() < len {
            match random.below(10) {
                0..=2 => chunk.extend_from_slice(FRAGMENTS[random.below(24) as usize]),
                3..=5 => chunk.push(b' ' + random.below(95) as u8),
                6 => chunk.extend_from_slice(random.below(300).to_string().as_bytes()),
                _ => chunk.push(random.below(256) as u8),
            }
        }
        vt.write(&chunk);
        written += chunk.len();
        match random.below(40) {
            0 => {
                let cols = 1 + random.below(200) as u16;
                let rows = 1 + random.below(60) as u16;
                vt.resize(cols, rows, 8, 16);
            }
            1 => vt.scroll(Scroll::Lines(random.below(50) as isize - 25)),
            2 => {
                let (cols, rows) = vt.size();
                let col = random.below(u64::from(cols)) as u16;
                let row = random.below(u64::from(rows)) as u16;
                vt.select(SelectStep::Press, point(0, 0, 0.0));
                vt.select(SelectStep::Drag, point(col, row, f32::from(col) * 10.0));
                vt.select(SelectStep::Release, point(col, row, f32::from(col) * 10.0));
                drop(vt.selection_text());
            }
            3 => {
                let _ = vt.search_tick();
                let _ = vt.search_next();
                let _ = vt.search_matches();
            }
            4 => {
                let bytes = vt.repaint(50);
                let (cols, rows) = vt.size();
                let mut copy = self::vt(cols, rows);
                copy.write(&bytes);
                let _ = copy.frame();
            }
            5 => {
                drop(vt.text(100));
                drop(vt.link_at(random.below(80) as u16, random.below(24) as u16));
                let _ = vt.mouse(&click(MouseAction::Press, MouseButton::Left, 30.0, 30.0));
                let _ = vt.key(&KeyInput::press(Key::ArrowUp));
            }
            _ => {}
        }
        if random.below(4) == 0 {
            let _ = vt.changed();
            let frame = vt.frame();
            let (cols, rows) = vt.size();
            assert_eq!((frame.cols, frame.rows), (cols, rows));
            assert_eq!(frame.lines.len(), usize::from(rows));
            assert!(
                frame
                    .lines
                    .iter()
                    .all(|line| line.cells.len() == usize::from(cols))
            );
            if let Some(cursor) = frame.cursor {
                assert!(cursor.x < cols && cursor.y < rows);
            }
        }
        let _ = vt.take_replies();
        let _ = vt.take_events();
    }
}

#[test]
fn moves_between_threads() {
    fn assert_send<T: Send>() {}
    assert_send::<Vt>();

    let mut vt = vt(20, 4);
    vt.write(b"main thread\r\n");
    let _ = vt.frame();
    let mut vt = std::thread::spawn(move || {
        vt.write(b"worker thread\x1b[6n\x07");
        assert_eq!(vt.take_replies(), b"\x1b[2;14R");
        let frame = vt.frame();
        assert_eq!(line(&frame, 1), "worker thread");
        vt
    })
    .join()
    .unwrap();
    assert_eq!(vt.take_events(), [Event::Bell]);
    assert_eq!(vt.text(0), "main thread\nworker thread");
    std::thread::spawn(move || drop(vt)).join().unwrap();
}
