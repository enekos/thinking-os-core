//! The backlog between whoever produces frames and the one thread that
//! writes them to a socket. Producers never touch the socket, so they can
//! push while holding state locks.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::util::LockExt as _;
use crate::wire::{Frame, Kind, write_frame};

#[derive(Debug, Default)]
struct Inner {
    frames: VecDeque<Frame>,
    /// Payload bytes queued, all kinds.
    bytes: usize,
    /// Payload bytes queued as `OUTPUT`, per terminal.
    output: HashMap<u32, usize>,
    closed: bool,
    /// The writer has taken a frame it has not finished writing.
    writing: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Push {
    Queued,
    /// Taking it would put the terminal's stream over the limit; nothing was
    /// queued.
    Overflow,
    Closed,
}

#[derive(Debug, Default)]
pub(crate) struct FrameQueue {
    inner: Mutex<Inner>,
    cond: Condvar,
}

impl FrameQueue {
    /// Queues a frame; `false` once closed.
    pub(crate) fn push(&self, frame: Frame) -> bool {
        let mut q = self.inner.locked();
        if q.closed {
            return false;
        }
        q.bytes += frame.payload.len();
        if frame.kind == Kind::Output {
            *q.output.entry(frame.terminal).or_default() += frame.payload.len();
        }
        q.frames.push_back(frame);
        drop(q);
        self.cond.notify_all();
        true
    }

    /// Queues a frame unless the queue already holds `limit` bytes.
    pub(crate) fn push_within(&self, frame: Frame, limit: usize) -> Push {
        let mut q = self.inner.locked();
        if q.closed {
            return Push::Closed;
        }
        if q.bytes + frame.payload.len() > limit {
            return Push::Overflow;
        }
        q.bytes += frame.payload.len();
        q.frames.push_back(frame);
        drop(q);
        self.cond.notify_all();
        Push::Queued
    }

    /// Queues terminal output unless that terminal's stream already holds
    /// `limit` bytes.
    pub(crate) fn push_output(&self, terminal: u32, bytes: &[u8], limit: usize) -> Push {
        let mut q = self.inner.locked();
        if q.closed {
            return Push::Closed;
        }
        let queued = q.output.entry(terminal).or_default();
        if *queued + bytes.len() > limit {
            return Push::Overflow;
        }
        *queued += bytes.len();
        q.bytes += bytes.len();
        q.frames
            .push_back(Frame::new(Kind::Output, terminal, bytes.to_vec()));
        drop(q);
        self.cond.notify_all();
        Push::Queued
    }

    /// Forgets the terminal's queued output and queues `replay` in its place.
    pub(crate) fn replace_output(&self, terminal: u32, replay: Vec<u8>) {
        let mut q = self.inner.locked();
        if q.closed {
            return;
        }
        Self::forget_output(&mut q, terminal);
        q.bytes += replay.len();
        q.output.insert(terminal, replay.len());
        q.frames
            .push_back(Frame::new(Kind::Output, terminal, replay));
        drop(q);
        self.cond.notify_all();
    }

    /// Forgets the terminal's queued output.
    pub(crate) fn drop_output(&self, terminal: u32) {
        Self::forget_output(&mut self.inner.locked(), terminal);
    }

    fn forget_output(q: &mut Inner, terminal: u32) {
        if let Some(dropped) = q.output.remove(&terminal)
            && dropped > 0
        {
            q.frames
                .retain(|f| !(f.kind == Kind::Output && f.terminal == terminal));
            q.bytes -= dropped;
        }
    }

    /// Queued bytes that are not terminal output.
    pub(crate) fn control_bytes(&self) -> usize {
        let q = self.inner.locked();
        q.bytes - q.output.values().sum::<usize>()
    }

    /// The next frame to write, waiting for one; `None` once closed and
    /// empty.
    fn pop(&self) -> Option<Frame> {
        let mut q = self.inner.locked();
        q.writing = false;
        self.cond.notify_all();
        loop {
            if let Some(frame) = q.frames.pop_front() {
                q.bytes -= frame.payload.len();
                if frame.kind == Kind::Output
                    && let Some(n) = q.output.get_mut(&frame.terminal)
                {
                    *n = n.saturating_sub(frame.payload.len());
                }
                q.writing = true;
                return Some(frame);
            }
            if q.closed {
                return None;
            }
            q = self
                .cond
                .wait(q)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// No more frames are accepted; the writer finishes what is queued.
    pub(crate) fn close(&self) {
        self.inner.locked().closed = true;
        self.cond.notify_all();
    }

    /// Closes and throws the backlog away.
    pub(crate) fn abort(&self) {
        let mut q = self.inner.locked();
        q.closed = true;
        q.frames.clear();
        q.output.clear();
        q.bytes = 0;
        drop(q);
        self.cond.notify_all();
    }

    /// Waits until everything queued has been written; `false` on timeout.
    pub(crate) fn wait_written(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut q = self.inner.locked();
        while !q.frames.is_empty() || q.writing {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            q = self
                .cond
                .wait_timeout(q, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        true
    }

    /// The writer thread's body: writes frames until the queue is closed and
    /// empty or the socket fails, then aborts the queue so producers stop.
    pub(crate) fn write_to(&self, mut sink: impl Write) {
        while let Some(frame) = self.pop() {
            if write_frame(&mut sink, &frame).is_err() {
                break;
            }
        }
        self.abort();
        self.inner.locked().writing = false;
        self.cond.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_limit_is_per_terminal_and_replace_drops_only_that_stream() {
        let q = FrameQueue::default();
        assert!(q.push(Frame::new(Kind::Reply, 0, vec![0; 10])));
        assert_eq!(q.push_output(1, &[1; 60], 100), Push::Queued);
        assert_eq!(q.push_output(2, &[2; 90], 100), Push::Queued);
        assert_eq!(q.push_output(1, &[1; 60], 100), Push::Overflow);
        q.replace_output(1, vec![9; 5]);
        assert_eq!(q.control_bytes(), 10);
        let kinds: Vec<_> = std::iter::from_fn(|| {
            let f = q.inner.locked().frames.pop_front()?;
            Some((f.kind, f.terminal, f.payload.len()))
        })
        .collect();
        assert_eq!(
            kinds,
            [
                (Kind::Reply, 0, 10),
                (Kind::Output, 2, 90),
                (Kind::Output, 1, 5)
            ]
        );
    }

    #[test]
    fn writer_drains_then_stops_on_close() {
        let q = FrameQueue::default();
        q.push(Frame::new(Kind::Event, 0, b"{}".to_vec()));
        q.push_output(3, b"abc", 100);
        q.close();
        assert!(!q.push(Frame::new(Kind::Event, 0, Vec::new())));
        let mut out = Vec::new();
        q.write_to(&mut out);
        assert_eq!(out.len(), 9 + 2 + 9 + 3);
        assert!(q.wait_written(Duration::from_millis(1)));
    }
}
