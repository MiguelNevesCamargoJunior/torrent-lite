use std::io::{Read, Write};
use torrent_lite::safe_storage::SafeRoot;

#[test]
fn creates_nested_file_inside_open_root() {
    let sandbox = tempfile::tempdir().unwrap();
    let root_path = sandbox.path().join("root");
    std::fs::create_dir(&root_path).unwrap();
    let root = SafeRoot::open(&root_path).unwrap();
    let mut file = root.create_new(&["data", "nested", "sample.bin"]).unwrap();
    file.write_all(b"piece").unwrap();
    let mut actual = Vec::new();
    std::fs::File::open(root_path.join("data/nested/sample.bin"))
        .unwrap()
        .read_to_end(&mut actual)
        .unwrap();
    assert_eq!(actual, b"piece");
}

#[test]
fn refuses_existing_file_and_unsafe_components() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = SafeRoot::open(sandbox.path()).unwrap();
    root.create_new(&["existing.bin"]).unwrap();
    assert!(root.create_new(&["existing.bin"]).is_err());
    for path in [&["..", "outside"][..], &["C:", "outside"][..], &["a/b"][..]] {
        assert!(root.create_new(path).is_err());
    }
}

#[test]
fn competing_creates_have_one_winner() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = std::sync::Arc::new(SafeRoot::open(sandbox.path()).unwrap());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || root.create_new(&["same.bin"]).is_ok())
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap() as u8)
            .sum::<u8>(),
        1
    );
}

#[cfg(unix)]
#[test]
fn rejects_parent_symlink_outside_root() {
    let sandbox = tempfile::tempdir().unwrap();
    let root_path = sandbox.path().join("root");
    let outside = sandbox.path().join("outside");
    std::fs::create_dir(&root_path).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root_path.join("linked")).unwrap();
    let root = SafeRoot::open(&root_path).unwrap();
    assert!(root.create_new(&["linked", "escape.bin"]).is_err());
    assert!(!outside.join("escape.bin").exists());
}

#[cfg(unix)]
#[test]
fn rejects_final_symlink_to_existing_file() {
    let sandbox = tempfile::tempdir().unwrap();
    let root_path = sandbox.path().join("root");
    std::fs::create_dir(&root_path).unwrap();
    let outside = sandbox.path().join("outside.bin");
    std::fs::write(&outside, b"unchanged").unwrap();
    std::os::unix::fs::symlink(&outside, root_path.join("linked.bin")).unwrap();
    let root = SafeRoot::open(&root_path).unwrap();
    assert!(root.create_new(&["linked.bin"]).is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"unchanged");
}

#[cfg(windows)]
#[test]
fn rejects_parent_junction_outside_root() {
    let sandbox = tempfile::tempdir().unwrap();
    let root_path = sandbox.path().join("root");
    let outside = sandbox.path().join("outside");
    std::fs::create_dir(&root_path).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let link = root_path.join("linked");
    let result = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let root = SafeRoot::open(&root_path).unwrap();
    assert!(root.create_new(&["linked", "escape.bin"]).is_err());
    assert!(!outside.join("escape.bin").exists());
}
