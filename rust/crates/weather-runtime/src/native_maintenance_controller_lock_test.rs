use super::*;
use std::{
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root(PathBuf);
impl Root {
    /// Create an independent prospective maintenance home.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "weather-controller-lock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}
impl Drop for Root {
    /// Remove only the disposable fixture tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Prove native and predecessor kernel flock ownership agree on the same inode.
#[test]
fn native_controller_lock_interoperates_with_predecessor_flock() {
    let root = Root::new();
    let lock = NativeMaintenanceControllerProcessLock::acquire_in_home(&root.0).unwrap();
    let state = root.0.join(STATE_SUFFIX.join("/"));
    let path = state.join("maintenance-controller.lock");
    let result = Command::new("/usr/bin/flock")
        .args(["--exclusive", "--nonblock"])
        .arg(&path)
        .arg("/usr/bin/true")
        .status()
        .unwrap();
    assert_eq!(result.code(), Some(1));
    drop(lock);
    let result = Command::new("/usr/bin/flock")
        .args(["--exclusive", "--nonblock"])
        .arg(&path)
        .arg("/usr/bin/true")
        .status()
        .unwrap();
    assert!(result.success());
    assert!(NativeMaintenanceControllerProcessLock::acquire_in_home(&root.0).is_ok());
}
