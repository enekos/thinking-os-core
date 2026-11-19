//! Session records on disk: `<state dir>/sessions/<id>.json`, one file per
//! session, written atomically so a reader never sees half a record.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::SessionRecord;

/// Records larger than this are not ours.
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_ID_CHARS: usize = 128;

/// `$ARTALDE_STATE_DIR`, else `~/.local/state/artalde`.
pub fn state_dir() -> PathBuf {
    std::env::var_os("ARTALDE_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| crate::home().map(|h| h.join(".local/state/artalde")))
        .unwrap_or_else(|| PathBuf::from(".artalde-state"))
}

#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(state_dir: &Path) -> Store {
        Store {
            dir: state_dir.join("sessions"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The file for `id`; ids are reduced to `[A-Za-z0-9_-]` so none can
    /// climb out of the directory.
    pub fn path(&self, id: &str) -> PathBuf {
        let safe: String = id
            .chars()
            .take(MAX_ID_CHARS)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    pub fn load(&self, id: &str) -> Option<SessionRecord> {
        read_record(&self.path(id))
    }

    pub fn save(&self, rec: &SessionRecord) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path(&rec.id);
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let body = serde_json::to_vec_pretty(rec).map_err(io::Error::other)?;
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        match fs::remove_file(self.path(id)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Every readable record; unreadable files are skipped.
    pub fn all(&self) -> Vec<SessionRecord> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter_map(|p| read_record(&p))
            .collect()
    }
}

fn read_record(path: &Path) -> Option<SessionRecord> {
    if fs::metadata(path).ok()?.len() > MAX_RECORD_BYTES {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentState;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("artalde-store-{name}-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn save_load_remove() {
        let store = Store::new(&scratch("slr"));
        let rec = SessionRecord {
            id: "abc-1".into(),
            state: AgentState::Blocked,
            ..Default::default()
        };
        store.save(&rec).unwrap();
        assert_eq!(store.load("abc-1"), Some(rec.clone()));
        assert_eq!(store.all(), vec![rec]);
        store.remove("abc-1").unwrap();
        store.remove("abc-1").unwrap();
        assert!(store.all().is_empty());
    }

    #[test]
    fn ids_cannot_escape() {
        let store = Store::new(Path::new("/s"));
        assert_eq!(
            store.path("../../etc/passwd"),
            PathBuf::from("/s/sessions/______etc_passwd.json")
        );
    }
}
