//! Name and working directory of another process.

use rustix::process::Pid;

/// The short name of the program `pid` runs.
pub(crate) fn name(pid: Pid) -> Option<String> {
    imp::name(pid).filter(|n| !n.is_empty())
}

/// The working directory of `pid`.
pub(crate) fn cwd(pid: Pid) -> Option<String> {
    imp::cwd(pid).filter(|p| p.starts_with('/'))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod imp {
    use rustix::process::Pid;

    pub(super) fn name(pid: Pid) -> Option<String> {
        let raw = pid.as_raw_nonzero();
        let comm = std::fs::read_to_string(format!("/proc/{raw}/comm")).ok()?;
        Some(comm.trim_end().to_owned())
    }

    pub(super) fn cwd(pid: Pid) -> Option<String> {
        let raw = pid.as_raw_nonzero();
        let path = std::fs::read_link(format!("/proc/{raw}/cwd")).ok()?;
        path.into_os_string().into_string().ok()
    }
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "libproc is the only way to ask macOS about another process"
)]
mod imp {
    use std::ffi::{CStr, c_int, c_void};
    use std::mem::{offset_of, size_of};

    use rustix::process::Pid;

    fn text(bytes: &[u8]) -> Option<String> {
        let text = CStr::from_bytes_until_nul(bytes).ok()?;
        Some(text.to_str().ok()?.to_owned())
    }

    pub(super) fn name(pid: Pid) -> Option<String> {
        // Twice MAXCOMLEN and a terminator, which is all the kernel keeps.
        let mut buf = [0u8; 64];
        let size = u32::try_from(buf.len() - 1).ok()?;
        // SAFETY: `buf` is writable for `size` bytes, one fewer than its
        // length, and `proc_name` writes at most that many.
        let written = unsafe {
            libc::proc_name(
                pid.as_raw_nonzero().get(),
                buf.as_mut_ptr().cast::<c_void>(),
                size,
            )
        };
        (written > 0).then(|| text(&buf)).flatten()
    }

    pub(super) fn cwd(pid: Pid) -> Option<String> {
        let len = size_of::<libc::proc_vnodepathinfo>();
        let mut buf = vec![0u8; len];
        let size = c_int::try_from(len).ok()?;
        // SAFETY: `buf` is writable for `size` bytes, the size of the
        // structure this flavor fills; the kernel copies bytes out and does
        // not rely on the buffer's alignment.
        let written = unsafe {
            libc::proc_pidinfo(
                pid.as_raw_nonzero().get(),
                libc::PROC_PIDVNODEPATHINFO,
                0,
                buf.as_mut_ptr().cast::<c_void>(),
                size,
            )
        };
        if written != size {
            return None;
        }
        let path = offset_of!(libc::proc_vnodepathinfo, pvi_cdir)
            + offset_of!(libc::vnode_info_path, vip_path);
        text(buf.get(path..)?)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
mod imp {
    use rustix::process::Pid;

    pub(super) fn name(_: Pid) -> Option<String> {
        None
    }

    pub(super) fn cwd(_: Pid) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn knows_this_process() {
        let me = rustix::process::getpid();
        let name = super::name(me).unwrap();
        assert!(name.starts_with("artalde"), "{name}");
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(super::cwd(me).as_deref(), cwd.to_str());
    }
}
