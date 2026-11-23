//! Pseudo-terminals.

use std::io;
use std::os::fd::{AsFd as _, BorrowedFd, OwnedFd};
use std::process::Stdio;

use rustix::fs::{Mode, OFlags};
use rustix::io::FdFlags;
use rustix::process::Pid;
use rustix::pty::OpenptFlags;
use rustix::termios::{InputModes, OptionalActions, Winsize};

/// Both ends of a pseudo-terminal.
///
/// The host keeps the slave end open for as long as the terminal lives: on
/// macOS the output a program wrote just before exiting is thrown away when
/// the last slave descriptor closes, and with ours open it stays readable.
/// The price is that the master never reports end of file, so the reader
/// learns of the exit from the child's waiter instead.
#[derive(Debug)]
pub(crate) struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
}

impl Pty {
    /// Opens a pair of the given size. The master is non-blocking and both
    /// ends are close-on-exec. The caller serialises this with spawning, as
    /// the master is briefly inheritable on platforms that cannot open it
    /// close-on-exec.
    pub(crate) fn open(cols: u16, rows: u16) -> io::Result<Pty> {
        let master = rustix::pty::openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
        rustix::io::fcntl_setfd(&master, FdFlags::CLOEXEC)?;
        rustix::pty::grantpt(&master)?;
        rustix::pty::unlockpt(&master)?;
        let name = rustix::pty::ptsname(&master, Vec::new())?;
        let slave = rustix::fs::open(
            name.as_c_str(),
            OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let flags = rustix::fs::fcntl_getfl(&master)?;
        rustix::fs::fcntl_setfl(&master, flags | OFlags::NONBLOCK)?;
        let pty = Pty { master, slave };
        pty.resize(cols, rows, 0, 0)?;
        // Tells the line discipline that erase removes a whole character.
        // Not every platform has the flag or lets it be set; nothing else
        // depends on it.
        if let Ok(mut termios) = rustix::termios::tcgetattr(&pty.slave) {
            termios.input_modes |= InputModes::IUTF8;
            rustix::termios::tcsetattr(&pty.slave, OptionalActions::Now, &termios).ok();
        }
        Ok(pty)
    }

    pub(crate) fn master(&self) -> BorrowedFd<'_> {
        self.master.as_fd()
    }

    /// The slave end three times over, for a child's standard streams.
    pub(crate) fn stdio(&self) -> io::Result<[Stdio; 3]> {
        Ok([
            Stdio::from(self.slave.try_clone()?),
            Stdio::from(self.slave.try_clone()?),
            Stdio::from(self.slave.try_clone()?),
        ])
    }

    pub(crate) fn resize(&self, cols: u16, rows: u16, xpixel: u16, ypixel: u16) -> io::Result<()> {
        let size = Winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: xpixel,
            ws_ypixel: ypixel,
        };
        Ok(rustix::termios::tcsetwinsize(&self.master, size)?)
    }

    /// The process group in the foreground of the terminal.
    pub(crate) fn foreground(&self) -> Option<Pid> {
        rustix::termios::tcgetpgrp(&self.master).ok()
    }
}
