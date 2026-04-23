//! Edge-case coverage for `PtyRegistry::create`. Particularly: creating a
//! virtual serial when a symlink at the chosen path already exists (maybe
//! dangling — e.g. left behind after a crash, or the PTY slave under it
//! was destroyed on shutdown).

#![cfg(unix)]

use std::path::PathBuf;

use modsim_server::transport::vpty::PtyRegistry;

fn tmp_link(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-vpty-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn create_without_symlink() {
    let reg = PtyRegistry::new();
    let info = reg.create(None).expect("create");
    assert!(info.slave_path.starts_with("/dev/"));
    assert!(info.symlink_path.is_none());
    assert!(!info.in_use);
    reg.remove(&info.id);
}

#[test]
fn create_with_fresh_symlink() {
    let reg = PtyRegistry::new();
    let link = tmp_link("fresh");
    let info = reg.create(Some(link.clone())).expect("create");
    assert_eq!(info.symlink_path.as_deref(), Some(link.as_path()));
    let target = std::fs::read_link(&link).expect("symlink exists");
    assert_eq!(target, info.slave_path);
    reg.remove(&info.id);
    assert!(!link.exists());
}

#[test]
fn create_replaces_existing_regular_symlink() {
    // A symlink pointing at a real file. Simulate a leftover from a prior
    // run whose target still happens to exist (e.g. pointing at /tmp).
    let reg = PtyRegistry::new();
    let link = tmp_link("replace-regular");
    let stash = tmp_link("replace-regular-target");
    std::fs::write(&stash, b"hi").unwrap();
    std::os::unix::fs::symlink(&stash, &link).unwrap();
    assert!(link.exists(), "precondition: link should resolve");

    let info = reg.create(Some(link.clone())).expect("create");
    let target = std::fs::read_link(&link).expect("symlink exists");
    assert_eq!(
        target, info.slave_path,
        "new symlink must point at the new slave, not the old target"
    );
    reg.remove(&info.id);
    let _ = std::fs::remove_file(&stash);
}

#[test]
fn create_replaces_dangling_symlink() {
    // This is the case that broke for the user: the symlink file still
    // exists on disk, but its target is gone (e.g. the old PTY slave
    // disappeared when the previous modsim process exited). `exists()`
    // follows symlinks and returns false for a dangling one — so a naive
    // existence check would skip removal and `symlink(2)` would then fail
    // with EEXIST.
    let reg = PtyRegistry::new();
    let link = tmp_link("dangling");
    std::os::unix::fs::symlink("/tmp/definitely-does-not-exist-XYZ", &link).unwrap();
    assert!(
        !link.exists(),
        "precondition: link is dangling (exists() returns false)"
    );
    assert!(
        link.symlink_metadata().is_ok(),
        "precondition: the symlink file itself IS on disk"
    );

    let info = reg
        .create(Some(link.clone()))
        .expect("create must succeed even with a dangling leftover");
    let target = std::fs::read_link(&link).expect("symlink exists");
    assert_eq!(target, info.slave_path);
    reg.remove(&info.id);
}

#[test]
fn create_replaces_regular_file_at_symlink_path() {
    // A real file (not a symlink) occupies the target path. We should
    // overwrite it rather than fail.
    let reg = PtyRegistry::new();
    let link = tmp_link("regular-file");
    std::fs::write(&link, b"stale contents").unwrap();
    let info = reg
        .create(Some(link.clone()))
        .expect("create must succeed when a regular file occupies the path");
    let md = link.symlink_metadata().unwrap();
    assert!(md.file_type().is_symlink(), "path should now be a symlink");
    reg.remove(&info.id);
}

#[test]
fn create_many_is_independent() {
    let reg = PtyRegistry::new();
    let a = reg.create(None).expect("a");
    let b = reg.create(None).expect("b");
    let c = reg.create(None).expect("c");
    assert_ne!(a.id, b.id);
    assert_ne!(b.id, c.id);
    assert_eq!(reg.list().len(), 3);
    reg.remove(&a.id);
    reg.remove(&b.id);
    reg.remove(&c.id);
    assert_eq!(reg.list().len(), 0);
}

#[test]
fn create_with_id_roundtrips_id() {
    let reg = PtyRegistry::new();
    let info = reg
        .create_with_id("custom-id-42".into(), None)
        .expect("create_with_id");
    assert_eq!(info.id, "custom-id-42");
    assert!(reg.get("custom-id-42").is_some());
    reg.remove(&info.id);
}
