//! Workspace paths, the per-workspace singleton lock, and lockfile I/O.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use continuum_core::LockFile;
use continuum_graph::GraphSnapshot;
use fs2::FileExt;

/// A resolved workspace root and the `.continuum` directory beneath it.
#[derive(Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Canonicalize the workspace path and ensure `.continuum/` exists.
    pub fn resolve(path: &Path) -> Result<Self> {
        let root = path
            .canonicalize()
            .with_context(|| format!("resolve workspace path: {}", path.display()))?;
        fs::create_dir_all(root.join(".continuum")).context("create .continuum dir")?;
        Ok(Self { root })
    }

    pub fn display(&self) -> String {
        self.root.display().to_string()
    }

    pub fn root_path(&self) -> PathBuf {
        self.root.clone()
    }

    fn continuum_dir(&self) -> PathBuf {
        self.root.join(".continuum")
    }

    pub fn db_path(&self) -> PathBuf {
        self.continuum_dir().join("continuum.db")
    }

    pub fn lockfile_path(&self) -> PathBuf {
        self.continuum_dir().join("daemon.lock")
    }

    fn singleton_path(&self) -> PathBuf {
        self.continuum_dir().join("daemon.singleton.lock")
    }

    /// Take the per-workspace singleton lock, held for the daemon's lifetime.
    /// `Ok(None)` means another daemon already owns this workspace.
    pub fn acquire_singleton(&self) -> Result<Option<File>> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.singleton_path())
            .context("open singleton lock")?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(file)),
            Err(_) => Ok(None),
        }
    }

    pub fn write_lockfile(&self, lock: &LockFile) -> Result<()> {
        let json = serde_json::to_string_pretty(lock).context("serialize lockfile")?;
        fs::write(self.lockfile_path(), json).context("write daemon.lock")?;
        Ok(())
    }

    pub fn remove_lockfile(&self) {
        let _ = fs::remove_file(self.lockfile_path());
    }

    fn snapshot_path(&self) -> PathBuf {
        self.continuum_dir().join("graph.json")
    }

    /// Load a previously written graph snapshot, if one exists and parses.
    pub fn read_snapshot(&self) -> Option<GraphSnapshot> {
        let data = fs::read_to_string(self.snapshot_path()).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Persist a graph snapshot, writing atomically via a temp file + rename
    /// so a crash mid-write cannot leave a corrupt snapshot.
    pub fn write_snapshot(&self, snapshot: &GraphSnapshot) {
        let json = match serde_json::to_string(snapshot) {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!("snapshot serialize failed: {e}");
                return;
            }
        };
        let tmp = self.continuum_dir().join("graph.json.tmp");
        if let Err(e) = fs::write(&tmp, json) {
            tracing::warn!("snapshot write failed: {e}");
            return;
        }
        if let Err(e) = fs::rename(&tmp, self.snapshot_path()) {
            tracing::warn!("snapshot rename failed: {e}");
        }
    }
}
