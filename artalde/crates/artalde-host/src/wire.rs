//! The wire protocol: frames, and the JSON records carried in them.
//!
//! A frame is `[kind u8][terminal u32 LE][len u32 LE][payload]`; `terminal`
//! is 0 on control frames. See `docs/terminals.md`.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// Bumped when a change would confuse the other side.
pub const PROTOCOL_VERSION: u32 = 1;
/// Largest payload either side accepts.
pub const MAX_PAYLOAD: usize = 16 << 20;
/// Bytes before the payload.
pub const HEADER_LEN: usize = 9;

/// What a frame carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    /// Client to host: `{"id", "method", "params"}`.
    Request = 1,
    /// Host to client: `{"id", "result"}` or `{"id", "error"}`.
    Reply = 2,
    /// Host to client, after `subscribe`: one [`Event`].
    Event = 3,
    /// Host to client: bytes for the viewer's emulator.
    Output = 10,
    /// Client to host: bytes for the PTY.
    Input = 11,
    /// Client to host: `cols, rows, cell_w, cell_h`, each `u16` LE.
    Resize = 12,
    /// Client to host: stop the stream. Empty.
    Detach = 13,
    /// Host to client: the program ended; `{"code", "signal"}`.
    Ended = 14,
}

impl TryFrom<u8> for Kind {
    type Error = FrameError;

    fn try_from(byte: u8) -> Result<Kind, FrameError> {
        Ok(match byte {
            1 => Kind::Request,
            2 => Kind::Reply,
            3 => Kind::Event,
            10 => Kind::Output,
            11 => Kind::Input,
            12 => Kind::Resize,
            13 => Kind::Detach,
            14 => Kind::Ended,
            other => return Err(FrameError::UnknownKind(other)),
        })
    }
}

/// One decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: Kind,
    pub terminal: u32,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(kind: Kind, terminal: u32, payload: Vec<u8>) -> Frame {
        Frame {
            kind,
            terminal,
            payload,
        }
    }

    pub(crate) fn json<T: Serialize>(kind: Kind, terminal: u32, value: &T) -> Frame {
        // Serialising the crate's own records cannot fail; an empty payload
        // would be rejected by the peer as malformed rather than misread.
        Frame::new(
            kind,
            terminal,
            serde_json::to_vec(value).unwrap_or_default(),
        )
    }
}

/// Why a frame could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FrameError {
    #[error("frame kind {0} is not known")]
    UnknownKind(u8),
    #[error("frame payload of {0} bytes is over the limit")]
    TooLarge(u32),
    #[error("connection closed inside a frame")]
    Truncated,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// The nine header bytes for a frame.
pub fn header(kind: Kind, terminal: u32, len: usize) -> io::Result<[u8; HEADER_LEN]> {
    let len = u32::try_from(len)
        .ok()
        .filter(|_| len <= MAX_PAYLOAD)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame payload too large"))?;
    let mut out = [0u8; HEADER_LEN];
    out[0] = kind as u8;
    out[1..5].copy_from_slice(&terminal.to_le_bytes());
    out[5..9].copy_from_slice(&len.to_le_bytes());
    Ok(out)
}

/// Writes one frame.
///
/// # Errors
/// The payload is over [`MAX_PAYLOAD`], or the writer failed.
pub fn write_frame(w: &mut impl Write, frame: &Frame) -> io::Result<()> {
    let head = header(frame.kind, frame.terminal, frame.payload.len())?;
    if frame.payload.len() <= 64 * 1024 {
        let mut whole = Vec::with_capacity(HEADER_LEN + frame.payload.len());
        whole.extend_from_slice(&head);
        whole.extend_from_slice(&frame.payload);
        w.write_all(&whole)
    } else {
        w.write_all(&head)?;
        w.write_all(&frame.payload)
    }
}

/// Reads one frame; `Ok(None)` when the stream ended between frames.
///
/// # Errors
/// An unknown kind, a length over [`MAX_PAYLOAD`], an end inside a frame, or
/// a read failure. After any of them the stream is out of step and must be
/// dropped.
pub fn read_frame(r: &mut impl Read) -> Result<Option<Frame>, FrameError> {
    let mut head = [0u8; HEADER_LEN];
    let mut got = 0;
    while got < HEADER_LEN {
        match r.read(&mut head[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    let kind = Kind::try_from(head[0])?;
    let terminal = u32::from_le_bytes([head[1], head[2], head[3], head[4]]);
    let len = u32::from_le_bytes([head[5], head[6], head[7], head[8]]);
    let size = usize::try_from(len).map_err(|_| FrameError::TooLarge(len))?;
    if size > MAX_PAYLOAD {
        return Err(FrameError::TooLarge(len));
    }
    let mut payload = vec![0u8; size];
    r.read_exact(&mut payload).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            FrameError::Truncated
        } else {
            FrameError::Io(e)
        }
    })?;
    Ok(Some(Frame {
        kind,
        terminal,
        payload,
    }))
}

/// Whether a terminal's program is alive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum State {
    Running,
    Exited {
        /// Exit code, when the program exited by itself.
        code: Option<i32>,
        /// Signal number, when a signal ended it.
        signal: Option<i32>,
        /// When, unix seconds.
        at: u64,
        /// The host that ran it went away before it could see the end, so
        /// neither `code` nor `signal` is known.
        #[serde(default)]
        lost: bool,
    },
}

impl State {
    pub fn is_running(&self) -> bool {
        matches!(self, State::Running)
    }
}

/// The states a program reports with `OSC 7501`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

/// What the program last said about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub state: StatusState,
    /// For `blocked`: `permission`, `question`, `auth`.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    /// Percent, 0 to 100.
    #[serde(default)]
    pub progress: Option<u8>,
    /// How many times the program has left `working`.
    pub turn: u32,
    /// When `state` last changed, unix seconds.
    pub since: u64,
}

/// Everything the host knows about one terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInfo {
    /// Never reused within a state directory.
    pub id: u32,
    pub name: String,
    pub argv: Vec<String>,
    pub cwd: String,
    /// The program's pid while it runs.
    pub pid: Option<u32>,
    pub cols: u16,
    pub rows: u16,
    /// Unix seconds.
    pub created: u64,
    pub state: State,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Name of the process in the foreground of the terminal.
    #[serde(default)]
    pub foreground: Option<String>,
    /// Attached clients right now.
    #[serde(default)]
    pub viewers: u32,
}

/// A change, sent to subscribers. Every value is absolute, so applying an
/// event twice, or one that a `list` already reflected, is harmless.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Event {
    Created {
        terminal: u32,
        info: Box<TerminalInfo>,
    },
    Exited {
        terminal: u32,
        code: Option<i32>,
        signal: Option<i32>,
        at: u64,
    },
    Removed {
        terminal: u32,
    },
    Title {
        terminal: u32,
        title: Option<String>,
    },
    Cwd {
        terminal: u32,
        cwd: String,
    },
    Status {
        terminal: u32,
        status: Option<Status>,
    },
    Bell {
        terminal: u32,
    },
    Renamed {
        terminal: u32,
        name: String,
    },
    Labels {
        terminal: u32,
        labels: BTreeMap<String, String>,
    },
    Size {
        terminal: u32,
        cols: u16,
        rows: u16,
    },
    Foreground {
        terminal: u32,
        name: Option<String>,
    },
}

impl Event {
    /// The terminal the event is about.
    pub fn terminal(&self) -> u32 {
        match self {
            Event::Created { terminal, .. }
            | Event::Exited { terminal, .. }
            | Event::Removed { terminal }
            | Event::Title { terminal, .. }
            | Event::Cwd { terminal, .. }
            | Event::Status { terminal, .. }
            | Event::Bell { terminal }
            | Event::Renamed { terminal, .. }
            | Event::Labels { terminal, .. }
            | Event::Size { terminal, .. }
            | Event::Foreground { terminal, .. } => *terminal,
        }
    }
}

/// The reply to `hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInfo {
    /// The host's [`PROTOCOL_VERSION`].
    pub version: u32,
    pub pid: u32,
    /// When the host started, unix seconds.
    pub started: u64,
}

/// Payload of an `ENDED` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ended {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// Payload of a `RESIZE` frame.
pub(crate) fn encode_resize(cols: u16, rows: u16, cell_w: u16, cell_h: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    for n in [cols, rows, cell_w, cell_h] {
        out.extend_from_slice(&n.to_le_bytes());
    }
    out
}

pub(crate) fn decode_resize(payload: &[u8]) -> Option<[u16; 4]> {
    let bytes: &[u8; 8] = payload.try_into().ok()?;
    Some([
        u16::from_le_bytes([bytes[0], bytes[1]]),
        u16::from_le_bytes([bytes[2], bytes[3]]),
        u16::from_le_bytes([bytes[4], bytes[5]]),
        u16::from_le_bytes([bytes[6], bytes[7]]),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(frame: &Frame) -> Vec<u8> {
        let mut out = Vec::new();
        write_frame(&mut out, frame).unwrap();
        out
    }

    #[test]
    fn frames_round_trip() {
        let frames = [
            Frame::new(Kind::Request, 0, br#"{"id":1,"method":"list"}"#.to_vec()),
            Frame::new(Kind::Output, 7, vec![0, 1, 2, 255]),
            Frame::new(Kind::Detach, u32::MAX, Vec::new()),
            Frame::new(Kind::Output, 3, vec![b'x'; 200_000]),
        ];
        let mut bytes = Vec::new();
        for f in &frames {
            bytes.extend(encode(f));
        }
        let mut r = bytes.as_slice();
        for f in &frames {
            assert_eq!(read_frame(&mut r).unwrap().as_ref(), Some(f));
        }
        assert!(read_frame(&mut r).unwrap().is_none());
    }

    #[test]
    fn header_layout_is_kind_terminal_length() {
        let bytes = encode(&Frame::new(Kind::Input, 0x0102_0304, b"ab".to_vec()));
        assert_eq!(bytes, [11, 4, 3, 2, 1, 2, 0, 0, 0, b'a', b'b']);
    }

    #[test]
    fn oversized_length_is_refused_without_allocating() {
        let mut bytes = vec![10];
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(u32::MAX.to_le_bytes());
        assert!(matches!(
            read_frame(&mut bytes.as_slice()),
            Err(FrameError::TooLarge(u32::MAX))
        ));
        let mut just_over = vec![10];
        just_over.extend(0u32.to_le_bytes());
        just_over.extend((MAX_PAYLOAD as u32 + 1).to_le_bytes());
        assert!(matches!(
            read_frame(&mut just_over.as_slice()),
            Err(FrameError::TooLarge(_))
        ));
        assert!(header(Kind::Output, 0, MAX_PAYLOAD + 1).is_err());
    }

    #[test]
    fn unknown_kind_is_refused() {
        for kind in [0u8, 4, 9, 15, 200] {
            let mut bytes = vec![kind];
            bytes.extend([0u8; 8]);
            assert!(matches!(
                read_frame(&mut bytes.as_slice()),
                Err(FrameError::UnknownKind(k)) if k == kind
            ));
        }
    }

    #[test]
    fn every_truncation_is_an_error_not_a_frame() {
        let whole = encode(&Frame::new(Kind::Request, 0, b"{\"id\":1}".to_vec()));
        for cut in 1..whole.len() {
            assert!(
                matches!(read_frame(&mut &whole[..cut]), Err(FrameError::Truncated)),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2000 {
            let mut bytes = Vec::new();
            for _ in 0..(seed % 40) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                bytes.push((seed & 0xff) as u8);
            }
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let mut r = bytes.as_slice();
            while let Ok(Some(_)) = read_frame(&mut r) {}
        }
    }

    #[test]
    fn records_have_the_documented_json_shape() {
        let ev = Event::Exited {
            terminal: 4,
            code: Some(0),
            signal: None,
            at: 9,
        };
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"type":"exited","terminal":4,"code":0,"signal":null,"at":9}"#
        );
        assert_eq!(
            serde_json::to_string(&State::Running).unwrap(),
            r#"{"kind":"running"}"#
        );
        let lost: State = serde_json::from_str(
            r#"{"kind":"exited","code":null,"signal":null,"at":1,"lost":true}"#,
        )
        .unwrap();
        assert_eq!(
            lost,
            State::Exited {
                code: None,
                signal: None,
                at: 1,
                lost: true
            }
        );
        assert_eq!(
            decode_resize(&encode_resize(80, 24, 9, 18)),
            Some([80, 24, 9, 18])
        );
        assert_eq!(decode_resize(&[0; 7]), None);
    }
}
