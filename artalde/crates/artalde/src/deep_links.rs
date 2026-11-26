//! Hand URLs to the running window. The lock also elects one window on concurrent cold starts.
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use artalde_core::{github::PrRef, store};

const MAX_URL: usize = 8192;
pub const POLL: Duration = Duration::from_millis(350);

pub struct Links {
    dir: PathBuf,
    _lock: File,
    last_read: Option<Instant>,
}

impl Links {
    /// Writing through a temporary name keeps partially delivered links invisible.
    pub fn connect(link: Option<&str>) -> anyhow::Result<Option<Self>> {
        let dir = store::state_dir().join("open-links");
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&dir)?;
        if let Some(link) = link {
            anyhow::ensure!(
                link.len() <= MAX_URL && PrRef::link(link).is_some(),
                "Expected a GitHub PR URL"
            );
            let name = format!("{}-{}", artalde_core::now(), std::process::id());
            let tmp = dir.join(format!("{name}.tmp"));
            File::create(&tmp)?.write_all(link.as_bytes())?;
            fs::rename(tmp, dir.join(format!("{name}.link")))?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("window.lock"))?;
        match lock.try_lock() {
            Ok(()) => Ok(Some(Self {
                dir,
                _lock: lock,
                last_read: None,
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(e.into()),
        }
    }

    pub fn take(&mut self) -> Vec<String> {
        if self.last_read.is_some_and(|t| t.elapsed() < POLL) {
            return Vec::new();
        }
        self.last_read = Some(Instant::now());
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "link"))
            .take(64)
            .collect();
        paths.sort();
        let mut links = Vec::new();
        for path in paths {
            let mut link = String::new();
            if File::open(&path)
                .and_then(|f| f.take(MAX_URL as u64 + 1).read_to_string(&mut link))
                .is_ok()
                && link.len() <= MAX_URL
                && PrRef::link(&link).is_some()
            {
                links.push(link);
            }
            if let Err(e) = fs::remove_file(&path) {
                eprintln!("artalde: consuming link: {e}");
            }
        }
        links
    }
}
