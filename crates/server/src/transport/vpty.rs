//! Virtual serial port pairs.
//!
//! On Unix (Linux + macOS) we use `openpty(3)` to create a master/slave
//! pseudo-terminal pair. We expose *two* user-visible endpoints by holding
//! the master side inside the simulator and creating two symlinks: one
//! pointing at the master fd's /dev/ttys* path, one at the slave side.
//!
//! Actually, a single PTY has exactly two endpoints (master + slave). For a
//! "loopback" style where the simulator reads/writes one end and a test app
//! uses the other, we keep the master fd in-process and expose the slave
//! path via a user-chosen symlink. The simulator's RTU serve loop uses the
//! master fd directly (wrapped in a [`SerialStream`]-like async adapter).
//!
//! On Windows this module compiles to stubs that return "not supported".

#![allow(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualPty {
    pub id: String,
    /// User-chosen symlink pointing at the slave endpoint (consumed by the
    /// user's test application).
    pub symlink_path: Option<PathBuf>,
    /// Underlying slave device path, e.g. `/dev/ttys003`.
    pub slave_path: PathBuf,
}

#[derive(Default)]
pub struct PtyRegistry {
    inner: Mutex<Vec<PtyEntry>>,
}

struct PtyEntry {
    info: VirtualPty,
    #[cfg(unix)]
    _master_fd: std::os::fd::OwnedFd,
}

impl PtyRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn list(&self) -> Vec<VirtualPty> {
        self.inner.lock().iter().map(|e| e.info.clone()).collect()
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut g = self.inner.lock();
        if let Some(pos) = g.iter().position(|e| e.info.id == id) {
            let e = g.remove(pos);
            if let Some(link) = &e.info.symlink_path {
                let _ = std::fs::remove_file(link);
            }
            return true;
        }
        false
    }

    #[cfg(unix)]
    pub fn create(&self, symlink: Option<PathBuf>) -> anyhow::Result<VirtualPty> {
        use std::os::fd::AsRawFd;

        // openpty fills master + slave fds. We own the master; the slave is
        // closed — the user's test app will open the slave device path.
        let result = nix::pty::openpty(None, None)?;
        let master_fd = result.master;
        let slave_fd = result.slave;
        // Resolve slave path via ptsname before dropping the slave.
        let slave_path = unsafe {
            let raw = master_fd.as_raw_fd();
            let cstr = libc::ptsname(raw);
            if cstr.is_null() {
                return Err(anyhow::anyhow!("ptsname failed"));
            }
            let path = std::ffi::CStr::from_ptr(cstr)
                .to_string_lossy()
                .into_owned();
            PathBuf::from(path)
        };
        drop(slave_fd);

        if let Some(link) = &symlink {
            if link.exists() {
                let _ = std::fs::remove_file(link);
            }
            std::os::unix::fs::symlink(&slave_path, link)?;
        }

        let info = VirtualPty {
            id: uuid::Uuid::new_v4().to_string(),
            symlink_path: symlink,
            slave_path,
        };
        self.inner.lock().push(PtyEntry {
            info: info.clone(),
            _master_fd: master_fd,
        });
        Ok(info)
    }

    #[cfg(not(unix))]
    pub fn create(&self, _symlink: Option<PathBuf>) -> anyhow::Result<VirtualPty> {
        Err(anyhow::anyhow!(
            "virtual PTYs are not supported on this platform"
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn create_pty_yields_real_slave_path() {
        let reg = PtyRegistry::new();
        let info = reg.create(None).expect("openpty");
        assert!(info.slave_path.starts_with("/dev/"));
        assert!(info.slave_path.exists());
        assert_eq!(reg.list().len(), 1);
        assert!(reg.remove(&info.id));
        assert_eq!(reg.list().len(), 0);
    }

    #[test]
    fn create_with_symlink_creates_link() {
        let reg = PtyRegistry::new();
        let link = std::env::temp_dir().join(format!("modsim-link-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        let info = reg.create(Some(link.clone())).expect("openpty");
        assert!(link.is_symlink() || link.exists());
        let target = std::fs::read_link(&link).expect("read_link");
        assert_eq!(target, info.slave_path);
        reg.remove(&info.id);
        assert!(!link.exists());
    }
}
