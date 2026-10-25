use std::io::{Read, Write};
use torrent_lite::safe_storage::{FileIdentity, SafeRoot};

#[test]
fn torrent_subdirectory_is_created_and_reopened_relative_to_root() {
    let workspace = tempfile::tempdir().unwrap();
    let root = SafeRoot::open(workspace.path()).unwrap();
    let first = root.subdir("torrent-1").unwrap();
    first.create_new(&["payload.bin"]).unwrap();
    let reopened = root.subdir("torrent-1").unwrap();
    assert!(reopened.create_new(&["other.bin"]).is_ok());
    assert!(
        workspace
            .path()
            .join("torrent-1")
            .join("payload.bin")
            .exists()
    );
    assert!(root.subdir("..").is_err());
    std::fs::write(workspace.path().join("occupied"), b"x").unwrap();
    assert!(root.subdir("occupied").is_err());
}

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

#[test]
fn reopens_only_the_file_with_the_recorded_identity() {
    let sandbox = tempfile::tempdir().unwrap();
    let root = SafeRoot::open(sandbox.path()).unwrap();
    let mut original = root.create_new(&["data.bin"]).unwrap();
    original.write_all(b"partial").unwrap();
    let identity = FileIdentity::from_file(&original).unwrap();
    drop(original);
    let mut reopened = root.open_existing(&["data.bin"], identity).unwrap();
    let mut data = Vec::new();
    reopened.read_to_end(&mut data).unwrap();
    assert_eq!(data, b"partial");

    let other = root.create_new(&["other.bin"]).unwrap();
    let other_identity = FileIdentity::from_file(&other).unwrap();
    assert!(root.open_existing(&["data.bin"], other_identity).is_err());
}

#[test]
fn refuses_a_hard_link_even_with_matching_identity() {
    let sandbox = tempfile::tempdir().unwrap();
    let root_path = sandbox.path().join("root");
    std::fs::create_dir(&root_path).unwrap();
    let root = SafeRoot::open(&root_path).unwrap();
    let original = root.create_new(&["data.bin"]).unwrap();
    let identity = FileIdentity::from_file(&original).unwrap();
    std::fs::hard_link(root_path.join("data.bin"), sandbox.path().join("alias.bin")).unwrap();
    assert!(root.open_existing(&["data.bin"], identity).is_err());
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
    assert!(root.subdir("linked").is_err());
    let outside_root = SafeRoot::open(&outside).unwrap();
    let outside_file = outside_root.create_new(&["present.bin"]).unwrap();
    let identity = FileIdentity::from_file(&outside_file).unwrap();
    assert!(
        root.open_existing(&["linked", "present.bin"], identity)
            .is_err()
    );
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
    let outside_file = cap_std::fs::File::from_std(std::fs::File::open(&outside).unwrap());
    let identity = FileIdentity::from_file(&outside_file).unwrap();
    assert!(root.open_existing(&["linked.bin"], identity).is_err());
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
    assert!(root.subdir("linked").is_err());
    let outside_root = SafeRoot::open(&outside).unwrap();
    let outside_file = outside_root.create_new(&["present.bin"]).unwrap();
    let identity = FileIdentity::from_file(&outside_file).unwrap();
    assert!(
        root.open_existing(&["linked", "present.bin"], identity)
            .is_err()
    );
    assert!(!outside.join("escape.bin").exists());
}
