//! The crate's one piece of `unsafe`: running code between fork and exec.

#![allow(
    unsafe_code,
    reason = "pre_exec is the only way to make a child a session leader"
)]

use std::io;
use std::os::unix::process::CommandExt as _;
use std::process::Command;

/// Makes the child a session leader, detached from the caller's terminal and
/// process group. With `controlling_tty`, its stdin (a PTY slave) becomes the
/// controlling terminal of the new session.
pub(crate) fn new_session(cmd: &mut Command, controlling_tty: bool) {
    let hook = move || -> io::Result<()> {
        rustix::process::setsid()?;
        if controlling_tty {
            rustix::process::ioctl_tiocsctty(rustix::stdio::stdin())?;
        }
        Ok(())
    };
    // SAFETY: the closure runs in the forked child before exec and makes only
    // the async-signal-safe `setsid` and `ioctl` system calls; it takes no
    // lock, allocates nothing, and captures one `bool` by value.
    unsafe {
        cmd.pre_exec(hook);
    }
}
