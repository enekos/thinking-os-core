//! Where the host keeps its files.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

/// `sun_path` is 104 bytes on macOS and 108 on Linux, NUL included.
const SOCKET_PATH_MAX: usize = 100;

pub(crate) fn lock_file(state: &Path) -> PathBuf {
    state.join("host.lock")
}

pub(crate) fn addr_file(state: &Path) -> PathBuf {
    state.join("host.addr")
}

pub(crate) fn registry_file(state: &Path) -> PathBuf {
    state.join("terminals.json")
}

pub(crate) fn seq_file(state: &Path) -> PathBuf {
    state.join("terminals.seq")
}

pub(crate) fn log_file(state: &Path) -> PathBuf {
    state.join("host.log")
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Where a host for `state` listens when nothing says otherwise:
/// `<state>/host.sock`, or, when that is too long for a socket address, a
/// directory named after a hash of `state` under `$TMPDIR` or `/tmp`.
fn default_socket(state: &Path) -> PathBuf {
    let direct = state.join("host.sock");
    if direct.as_os_str().len() <= SOCKET_PATH_MAX {
        return direct;
    }
    let absolute = fs::canonicalize(state).unwrap_or_else(|_| state.to_path_buf());
    let name = format!(
        "artalde-{}-{:016x}",
        rustix::process::getuid().as_raw(),
        fnv1a(absolute.as_os_str().as_encoded_bytes())
    );
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .filter(|t| t.is_absolute())
        .map(|t| t.join(&name).join("host.sock"))
        .filter(|p| p.as_os_str().len() <= SOCKET_PATH_MAX);
    tmp.unwrap_or_else(|| Path::new("/tmp").join(name).join("host.sock"))
}

/// The socket of the host for `state`: what the running host wrote to
/// `host.addr`, else the default.
pub fn socket_path(state: &Path) -> PathBuf {
    fs::read_to_string(addr_file(state))
        .ok()
        .map(|s| PathBuf::from(s.trim_end_matches('\n')))
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| default_socket(state))
}

/// Creates `dir` for this user only, or checks that an existing one is.
fn private_dir(dir: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(dir)?;
            if !meta.is_dir() || meta.uid() != rustix::process::getuid().as_raw() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{} is not a directory owned by this user", dir.display()),
                ));
            }
            if meta.permissions().mode() & 0o077 != 0 {
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Makes the state directory private and returns it.
pub(crate) fn prepare_state(state: &Path) -> io::Result<()> {
    if let Some(parent) = state.parent() {
        fs::create_dir_all(parent)?;
    }
    private_dir(state)
}

/// The path the host binds, with its directory made private.
pub(crate) fn socket_for_host(state: &Path) -> io::Result<PathBuf> {
    let path = default_socket(state);
    if let Some(dir) = path.parent()
        && dir != state
    {
        private_dir(dir)?;
    }
    Ok(path)
}

/// Writes `bytes` to `path` so that a reader sees the old or the new file,
/// never a partial one.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        fs::remove_file(&tmp).ok();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_state_dirs_get_a_short_socket() {
        let short = Path::new("/tmp/artalde-state");
        assert_eq!(default_socket(short), short.join("host.sock"));
        let long = PathBuf::from(format!("/tmp/{}", "x".repeat(120)));
        let sock = default_socket(&long);
        assert!(sock.as_os_str().len() <= SOCKET_PATH_MAX, "{sock:?}");
        assert_eq!(sock, default_socket(&long));
        assert_ne!(sock, default_socket(&long.join("other")));
        assert!(sock.ends_with("host.sock"));
    }
}
