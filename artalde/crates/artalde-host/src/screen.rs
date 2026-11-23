//! What the host knows about a terminal's display.
//!
//! This is the stand-in until the `artalde-vt` emulator is wired in: a ring
//! of the last output and a small scanner for the few sequences the host
//! acts on. The rest of the host uses only the methods of [`Screen`], so the
//! emulator can replace the inside without touching it.

use std::collections::VecDeque;

use crate::wire::StatusState;

/// Raw output kept per terminal.
const RING_BYTES: usize = 2 << 20;
/// Replay size for a viewer that did not ask for history.
const RECENT_BYTES: usize = 128 << 10;
/// Longest `OSC` payload the scanner keeps; longer ones are skipped.
const OSC_MAX: usize = 16 << 10;
/// Longest `CSI` parameter string the scanner keeps.
const CSI_MAX: usize = 64;
const TITLE_MAX: usize = 1024;

const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// Something in the output the host acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenEvent {
    /// `OSC 0` or `OSC 2`.
    Title(String),
    /// `OSC 7`: the path of the working directory.
    Cwd(String),
    Bell,
    /// `OSC 7501`.
    Status(StatusReport),
}

/// One `OSC 7501` record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusReport {
    /// `None` is `state=clear`.
    pub state: Option<StatusState>,
    pub kind: Option<String>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
    /// Percent, 0 to 100.
    pub progress: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scan {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    /// `DCS`, `APC`, `PM`, `SOS`: skipped up to the string terminator.
    Str,
    StrEsc,
}

/// A terminal's display state, fed with everything the program writes.
#[derive(Debug)]
pub struct Screen {
    cols: u16,
    rows: u16,
    ring: VecDeque<u8>,
    /// Output older than the ring has been dropped.
    truncated: bool,
    scan: Scan,
    seq: Vec<u8>,
    seq_overflow: bool,
    bracketed_paste: bool,
    alt_screen: bool,
    title: Option<String>,
}

impl Screen {
    pub fn new(cols: u16, rows: u16) -> Screen {
        Screen {
            cols,
            rows,
            ring: VecDeque::new(),
            truncated: false,
            scan: Scan::Ground,
            seq: Vec::new(),
            seq_overflow: false,
            bracketed_paste: false,
            alt_screen: false,
            title: None,
        }
    }

    /// Takes program output, in order, split anywhere.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<ScreenEvent> {
        let mut events = Vec::new();
        for &b in bytes {
            self.step(b, &mut events);
        }
        if bytes.len() >= RING_BYTES {
            self.ring.clear();
            self.truncated = true;
            self.ring
                .extend(&bytes[bytes.len().saturating_sub(RING_BYTES)..]);
        } else {
            let over = (self.ring.len() + bytes.len()).saturating_sub(RING_BYTES);
            if over > 0 {
                self.ring.drain(..over);
                self.truncated = true;
            }
            self.ring.extend(bytes);
        }
        events
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
    }

    /// The bytes a newly attached viewer writes into its emulator to arrive
    /// at this screen. Self-contained: it cancels any half-parsed sequence
    /// and resets the viewer first, so it is also what a viewer that fell
    /// behind is sent in place of its backlog.
    pub fn replay(&self, history: bool) -> Vec<u8> {
        let limit = if history { RING_BYTES } else { RECENT_BYTES };
        let len = self.ring.len();
        let mut start = len.saturating_sub(limit);
        if start > 0 || self.truncated {
            // The cut may fall inside a line or a sequence; a line boundary
            // is the closest thing to a clean start raw bytes offer.
            if let Some(nl) = self.ring.iter().skip(start).position(|&b| b == b'\n') {
                start += nl + 1;
            }
        }
        let mut out = Vec::with_capacity(len - start + 64);
        out.extend_from_slice(&[CAN, ESC, b'c']);
        // Whatever the tail toggles, it ends in the current mode; when it
        // toggles nothing the mode it started in is the current one too.
        if self.alt_screen {
            out.extend_from_slice(b"\x1b[?1049h");
        }
        if self.bracketed_paste {
            out.extend_from_slice(b"\x1b[?2004h");
        }
        if let Some(title) = &self.title {
            out.extend_from_slice(b"\x1b]2;");
            out.extend_from_slice(title.as_bytes());
            out.extend_from_slice(b"\x1b\\");
        }
        let (a, b) = self.ring.as_slices();
        if start < a.len() {
            out.extend_from_slice(&a[start..]);
            out.extend_from_slice(b);
        } else {
            out.extend_from_slice(&b[start - a.len()..]);
        }
        out
    }

    /// The last `lines` lines as plain text.
    pub fn text(&self, lines: usize) -> String {
        let mut plain = Plain::default();
        let (a, b) = self.ring.as_slices();
        for &byte in a.iter().chain(b) {
            plain.step(byte);
        }
        plain.finish(lines)
    }

    /// Answers to queries the program made of the terminal (device
    /// attributes and the like), to be written back to it.
    pub fn take_replies(&mut self) -> Vec<u8> {
        Vec::new()
    }

    /// The program asked for pastes to be bracketed (DEC mode 2004).
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    /// The program is on the alternate screen: a full-screen application.
    pub fn alt_screen(&self) -> bool {
        self.alt_screen
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    fn step(&mut self, b: u8, events: &mut Vec<ScreenEvent>) {
        match self.scan {
            Scan::Ground => match b {
                ESC => self.scan = Scan::Esc,
                BEL => events.push(ScreenEvent::Bell),
                _ => {}
            },
            Scan::Esc => self.escape(b),
            Scan::Csi => match b {
                0x20..=0x3f => {
                    if self.seq.len() < CSI_MAX {
                        self.seq.push(b);
                    } else {
                        self.seq_overflow = true;
                    }
                }
                0x40..=0x7e => {
                    self.scan = Scan::Ground;
                    if !self.seq_overflow {
                        self.csi(b);
                    }
                }
                ESC => self.scan = Scan::Esc,
                CAN | SUB => self.scan = Scan::Ground,
                BEL => events.push(ScreenEvent::Bell),
                _ => {}
            },
            Scan::Osc => match b {
                BEL => {
                    self.scan = Scan::Ground;
                    self.osc(events);
                }
                ESC => self.scan = Scan::OscEsc,
                CAN | SUB => self.scan = Scan::Ground,
                _ => {
                    if self.seq.len() < OSC_MAX {
                        self.seq.push(b);
                    } else {
                        self.seq_overflow = true;
                    }
                }
            },
            Scan::OscEsc => {
                if b == b'\\' {
                    self.scan = Scan::Ground;
                    self.osc(events);
                } else {
                    self.escape(b);
                }
            }
            Scan::Str => match b {
                ESC => self.scan = Scan::StrEsc,
                CAN | SUB => self.scan = Scan::Ground,
                _ => {}
            },
            Scan::StrEsc => {
                if b == b'\\' {
                    self.scan = Scan::Ground;
                } else {
                    self.escape(b);
                }
            }
        }
    }

    /// The byte after `ESC`.
    fn escape(&mut self, b: u8) {
        self.seq.clear();
        self.seq_overflow = false;
        self.scan = match b {
            b'[' => Scan::Csi,
            b']' => Scan::Osc,
            b'P' | b'_' | b'^' | b'X' => Scan::Str,
            ESC => Scan::Esc,
            b'c' => {
                self.bracketed_paste = false;
                self.alt_screen = false;
                Scan::Ground
            }
            _ => Scan::Ground,
        };
    }

    fn csi(&mut self, last: u8) {
        let set = match last {
            b'h' => true,
            b'l' => false,
            _ => return,
        };
        let Some(params) = self.seq.strip_prefix(b"?") else {
            return;
        };
        for param in params.split(|&b| b == b';') {
            match param {
                b"2004" => self.bracketed_paste = set,
                b"1049" | b"1047" | b"47" => self.alt_screen = set,
                _ => {}
            }
        }
    }

    fn osc(&mut self, events: &mut Vec<ScreenEvent>) {
        if self.seq_overflow {
            return;
        }
        let (code, body) = match self.seq.iter().position(|&b| b == b';') {
            Some(i) => (&self.seq[..i], &self.seq[i + 1..]),
            None => (self.seq.as_slice(), &[][..]),
        };
        match code {
            b"0" | b"2" => {
                let title = clean(&String::from_utf8_lossy(body), TITLE_MAX);
                self.title = (!title.is_empty()).then(|| title.clone());
                events.push(ScreenEvent::Title(title));
            }
            b"7" => {
                if let Some(path) = cwd_from_osc7(body) {
                    events.push(ScreenEvent::Cwd(path));
                }
            }
            b"7501" => {
                if let Some(report) = parse_status(body) {
                    events.push(ScreenEvent::Status(report));
                }
            }
            _ => {}
        }
    }
}

/// Text without control characters, at most `max` characters.
fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// `file://host/path` with percent-escapes, or a bare absolute path.
fn cwd_from_osc7(body: &[u8]) -> Option<String> {
    let path = match body.strip_prefix(b"file://") {
        Some(rest) => &rest[rest.iter().position(|&b| b == b'/')?..],
        None => body,
    };
    if path.first() != Some(&b'/') {
        return None;
    }
    let mut out = Vec::with_capacity(path.len());
    let mut i = 0;
    while let Some(&b) = path.get(i) {
        let escaped = (b == b'%')
            .then(|| path.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(b);
                i += 1;
            }
        }
    }
    let path = String::from_utf8(out).ok()?;
    (!path.chars().any(char::is_control)).then_some(path)
}

/// `state=working:kind=…:app=…:title=<b64>:msg=<b64>`. Records for a
/// sub-task (`id=` not empty) are not the terminal's status and give `None`.
fn parse_status(body: &[u8]) -> Option<StatusReport> {
    let body = std::str::from_utf8(body).ok()?;
    let mut report = StatusReport::default();
    let mut state = None;
    for field in body.split(':') {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        match key {
            "state" => state = Some(value),
            "id" => return None,
            "kind" => report.kind = Some(clean(value, 64)),
            "app" => report.app = Some(clean(value, 64)),
            "title" => report.title = text_field(value, 256),
            "msg" => report.message = text_field(value, 2048),
            "progress" => {
                report.progress = value
                    .trim_end_matches('%')
                    .parse::<f32>()
                    .ok()
                    .filter(|p| p.is_finite())
                    .map(|p| {
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "clamped to 0..=100 first"
                        )]
                        let percent = p.clamp(0.0, 100.0).round() as u8;
                        percent
                    });
            }
            _ => {}
        }
    }
    report.state = match state? {
        "idle" => Some(StatusState::Idle),
        "working" => Some(StatusState::Working),
        "done" => Some(StatusState::Done),
        "blocked" => Some(StatusState::Blocked),
        "error" => Some(StatusState::Error),
        "clear" => None,
        _ => return None,
    };
    Some(report)
}

fn text_field(b64: &str, max: usize) -> Option<String> {
    let bytes = base64(b64)?;
    let text = clean(&String::from_utf8_lossy(&bytes), max);
    (!text.is_empty()).then_some(text)
}

/// Standard or URL-safe alphabet, padding optional.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// Output with the escape sequences taken out, as lines.
#[derive(Debug, Default)]
struct Plain {
    lines: Vec<Vec<u8>>,
    line: Vec<u8>,
    /// A carriage return was seen: the next printable byte starts the line
    /// over, which is how progress bars redraw.
    returned: bool,
    scan: Option<Scan>,
}

impl Plain {
    fn step(&mut self, b: u8) {
        match self.scan.unwrap_or(Scan::Ground) {
            Scan::Ground => match b {
                ESC => self.scan = Some(Scan::Esc),
                b'\n' => {
                    self.returned = false;
                    self.lines.push(std::mem::take(&mut self.line));
                }
                b'\r' => self.returned = true,
                0x08 => {
                    self.line.pop();
                }
                b'\t' => self.put(b),
                0..=0x1f | 0x7f => {}
                _ => self.put(b),
            },
            Scan::Esc => {
                self.scan = Some(match b {
                    b'[' => Scan::Csi,
                    b']' => Scan::Osc,
                    b'P' | b'_' | b'^' | b'X' => Scan::Str,
                    ESC => Scan::Esc,
                    // Two-byte sequences that take one more byte: charset
                    // selection and the like.
                    b'(' | b')' | b'*' | b'+' | b'#' | b'%' | b' ' => Scan::OscEsc,
                    _ => Scan::Ground,
                });
            }
            // Reused here as "skip one byte".
            Scan::OscEsc => self.scan = Some(Scan::Ground),
            Scan::Csi => match b {
                0x40..=0x7e | CAN | SUB => self.scan = Some(Scan::Ground),
                ESC => self.scan = Some(Scan::Esc),
                _ => {}
            },
            Scan::Osc | Scan::Str => match b {
                BEL if self.scan == Some(Scan::Osc) => self.scan = Some(Scan::Ground),
                CAN | SUB => self.scan = Some(Scan::Ground),
                ESC => self.scan = Some(Scan::StrEsc),
                _ => {}
            },
            Scan::StrEsc => {
                self.scan = Some(Scan::Ground);
                if b != b'\\' {
                    self.scan = Some(Scan::Esc);
                    self.step(b);
                }
            }
        }
    }

    fn put(&mut self, b: u8) {
        if std::mem::take(&mut self.returned) {
            self.line.clear();
        }
        self.line.push(b);
    }

    fn finish(mut self, lines: usize) -> String {
        if !self.line.is_empty() {
            self.lines.push(self.line);
        }
        let from = self.lines.len().saturating_sub(lines);
        let mut out = String::new();
        for (i, line) in self.lines[from..].iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(String::from_utf8_lossy(line).trim_end());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `bytes` whole, byte by byte, and split at every position; all
    /// must report the same events.
    fn feed_all_ways(bytes: &[u8]) -> Vec<ScreenEvent> {
        let mut whole = Screen::new(80, 24);
        let expected = whole.feed(bytes);
        let mut single = Screen::new(80, 24);
        let mut got = Vec::new();
        for b in bytes {
            got.extend(single.feed(std::slice::from_ref(b)));
        }
        assert_eq!(got, expected, "byte by byte");
        for cut in 0..=bytes.len() {
            let mut s = Screen::new(80, 24);
            let mut got = s.feed(&bytes[..cut]);
            got.extend(s.feed(&bytes[cut..]));
            assert_eq!(got, expected, "split at {cut}");
            assert_eq!(s.bracketed_paste(), whole.bracketed_paste());
            assert_eq!(s.alt_screen(), whole.alt_screen());
        }
        expected
    }

    #[test]
    fn titles_with_either_terminator() {
        assert_eq!(
            feed_all_ways(b"a\x1b]0;one\x07b\x1b]2;two \xc3\xa9\x1b\\c\x1b]1;icon\x07"),
            [
                ScreenEvent::Title("one".into()),
                ScreenEvent::Title("two é".into())
            ]
        );
    }

    #[test]
    fn bell_only_outside_strings() {
        assert_eq!(
            feed_all_ways(b"\x07\x1b]0;t\x07\x1bPq\x07\x1b\\\x07"),
            [
                ScreenEvent::Bell,
                ScreenEvent::Title("t".into()),
                ScreenEvent::Bell
            ]
        );
    }

    #[test]
    fn osc7_cwd() {
        assert_eq!(
            feed_all_ways(b"\x1b]7;file://host.local/Users/me/my%20dir\x07\x1b]7;/plain\x1b\\\x1b]7;relative\x07\x1b]7;file://nohost\x07"),
            [
                ScreenEvent::Cwd("/Users/me/my dir".into()),
                ScreenEvent::Cwd("/plain".into())
            ]
        );
    }

    #[test]
    fn status_records() {
        let events = feed_all_ways(
            b"\x1b]7501;state=working:app=claude:title=aGVsbG8=:msg=d29ybGQh:progress=42\x1b\\\
              \x1b]7501;state=blocked:kind=permission\x07\
              \x1b]7501;state=clear\x07\
              \x1b]7501;state=working:id=sub1\x07\
              \x1b]7501;state=nonsense\x07\
              \x1b]7501;kind=question\x07",
        );
        assert_eq!(
            events,
            [
                ScreenEvent::Status(StatusReport {
                    state: Some(StatusState::Working),
                    kind: None,
                    app: Some("claude".into()),
                    title: Some("hello".into()),
                    message: Some("world!".into()),
                    progress: Some(42),
                }),
                ScreenEvent::Status(StatusReport {
                    state: Some(StatusState::Blocked),
                    kind: Some("permission".into()),
                    ..StatusReport::default()
                }),
                ScreenEvent::Status(StatusReport::default()),
            ]
        );
    }

    #[test]
    fn modes_are_tracked() {
        let mut s = Screen::new(80, 24);
        s.feed(b"\x1b[?2004h");
        assert!(s.bracketed_paste());
        s.feed(b"\x1b[?1049;2004l");
        assert!(!s.bracketed_paste());
        feed_all_ways(b"\x1b[?25l\x1b[?1049h\x1b[?2004h\x1b[2004l\x1b[1;31m");
        let mut s = Screen::new(80, 24);
        s.feed(b"\x1b[?1049h\x1b[?2004h\x1b[2004l");
        assert!(s.alt_screen() && s.bracketed_paste());
        s.feed(b"\x1bc");
        assert!(!s.alt_screen() && !s.bracketed_paste());
    }

    #[test]
    fn aborted_and_oversized_sequences_do_not_leak() {
        assert_eq!(
            feed_all_ways(b"\x1b]0;lost\x18\x1b]0;kept\x07\x1b]0;half\x1b[0m\x1b]2;next\x07"),
            [
                ScreenEvent::Title("kept".into()),
                ScreenEvent::Title("next".into())
            ]
        );
        let mut s = Screen::new(80, 24);
        let mut big = b"\x1b]0;".to_vec();
        big.extend(vec![b'x'; OSC_MAX + 10]);
        big.push(BEL);
        big.extend(b"\x1b]0;ok\x07");
        assert_eq!(s.feed(&big), [ScreenEvent::Title("ok".into())]);
    }

    #[test]
    fn replay_starts_on_a_line_once_output_was_dropped() {
        let mut s = Screen::new(80, 24);
        s.feed(b"first\nsecond\n");
        assert_eq!(s.replay(true), b"\x18\x1bcfirst\nsecond\n");
        let line = b"0123456789abcdef\n";
        for _ in 0..(RING_BYTES / line.len() + 100) {
            s.feed(line);
        }
        let replay = s.replay(true);
        assert!(replay.len() <= RING_BYTES + 3);
        assert!(replay.len() > RING_BYTES - 64);
        assert!(replay.starts_with(b"\x18\x1bc0123456789abcdef\n"));
        let recent = s.replay(false);
        assert!(recent.len() <= RECENT_BYTES + 3 && recent.len() > RECENT_BYTES - 64);
        assert!(recent.starts_with(b"\x18\x1bc0123456789abcdef\n"));
        assert!(recent.ends_with(b"abcdef\n"));
    }

    #[test]
    fn replay_restores_modes_and_title() {
        let mut s = Screen::new(80, 24);
        s.feed(b"\x1b[?1049h\x1b[?2004h\x1b]2;vim\x07body");
        let replay = s.replay(false);
        assert!(replay.starts_with(b"\x18\x1bc\x1b[?1049h\x1b[?2004h\x1b]2;vim\x1b\\"));
        assert!(replay.ends_with(b"body"));
    }

    #[test]
    fn one_feed_larger_than_the_ring() {
        let mut s = Screen::new(80, 24);
        s.feed(b"old\n");
        let mut big = vec![b'a'; RING_BYTES + 5000];
        big.extend(b"\ntail\n");
        s.feed(&big);
        assert_eq!(s.text(1), "tail");
        assert_eq!(s.replay(true), b"\x18\x1bctail\n");
    }

    #[test]
    fn text_strips_sequences() {
        let mut s = Screen::new(80, 24);
        s.feed(b"\x1b]0;title\x07\x1b[1;32mgreen\x1b[0m line\r\n10%\r50%\r100%\r\nab\x08c\x1b(B\r\n\x1bPdcs\x1b\\last  ");
        assert_eq!(s.text(10), "green line\n100%\nac\nlast");
        assert_eq!(s.text(2), "ac\nlast");
        assert_eq!(s.text(0), "");
        assert!(s.take_replies().is_empty());
    }

    #[test]
    fn base64_variants() {
        assert_eq!(base64("aGk").unwrap(), b"hi");
        assert_eq!(base64("aGk=").unwrap(), b"hi");
        assert_eq!(base64("_-8").unwrap(), [0xff, 0xef]);
        assert!(base64("a b").is_none());
    }
}
