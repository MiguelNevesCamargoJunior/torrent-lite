use torrent_lite::{
    InputError,
    path_policy::{validate_components, validate_file_set},
};

#[test]
fn ordinary_nested_unicode_paths_are_accepted() {
    assert_eq!(
        validate_components(&["Ubuntu", "imagens", "São Paulo.iso"]),
        Ok(())
    );
    assert_eq!(validate_components(&["auxiliary.txt"]), Ok(()));
}

#[test]
fn unsafe_windows_and_posix_components_are_rejected() {
    for name in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "C:",
        "\\\\server",
        "/root",
        "file:stream",
        "NUL",
        "con.txt",
        "COM1.iso",
        "lpt9",
        "COM¹.txt",
        "LPT²",
        "CONIN$",
        "CONOUT$",
        "a.",
        "a ",
        "a\0b",
        "a\nb",
        "a?b",
        "a*b",
        "a<b",
        "a>b",
        "a|b",
        "a\"b",
    ] {
        assert_eq!(
            validate_components(&["safe", name]),
            Err(InputError::UnsafePath),
            "{name:?}"
        );
    }
    assert_eq!(validate_components(&[]), Err(InputError::UnsafePath));
}

#[test]
fn path_depth_and_component_length_are_bounded() {
    assert!(validate_components(&vec!["a"; 32]).is_ok());
    assert_eq!(
        validate_components(&vec!["a"; 33]),
        Err(InputError::UnsafePath)
    );
    assert!(validate_components(&[&"a".repeat(255)]).is_ok());
    assert_eq!(
        validate_components(&[&"a".repeat(256)]),
        Err(InputError::UnsafePath)
    );
    assert_eq!(
        validate_components(&[&"🦀".repeat(128)]),
        Err(InputError::UnsafePath)
    );
}

fn files(paths: &[&[&str]]) -> Vec<Vec<String>> {
    paths
        .iter()
        .map(|p| p.iter().map(|v| (*v).to_owned()).collect())
        .collect()
}

#[test]
fn file_sets_reject_duplicate_case_and_normalization_aliases() {
    for paths in [
        files(&[&["a"], &["a"]]),
        files(&[&["Dir", "a"], &["dir", "A"]]),
        files(&[&["café"], &["cafe\u{301}"]]),
    ] {
        assert_eq!(validate_file_set(&paths), Err(InputError::PathCollision));
    }
    assert!(validate_file_set(&files(&[&["dir", "a"], &["dir", "b"]])).is_ok());
}

#[test]
fn file_directory_conflicts_are_rejected_in_both_orders() {
    assert_eq!(
        validate_file_set(&files(&[&["a"], &["a", "b"]])),
        Err(InputError::PathCollision)
    );
    assert_eq!(
        validate_file_set(&files(&[&["a", "b"], &["a"]])),
        Err(InputError::PathCollision)
    );
}

#[test]
fn file_set_size_is_bounded_and_components_are_checked() {
    let paths: Vec<_> = (0..10_000).map(|i| vec![i.to_string()]).collect();
    assert!(validate_file_set(&paths).is_ok());
    let mut excess = paths;
    excess.push(vec!["extra".into()]);
    assert_eq!(validate_file_set(&excess), Err(InputError::TooLarge));
    assert_eq!(
        validate_file_set(&files(&[&["..", "escape"]])),
        Err(InputError::UnsafePath)
    );
    assert_eq!(validate_file_set(&[]), Err(InputError::UnsafePath));
}

#[test]
fn aggregate_path_bytes_are_bounded_before_collision_allocation() {
    // Valid individual names must not turn 10,000 deep paths into huge prefix sets.
    let paths: Vec<Vec<String>> = (0..1100)
        .map(|i| {
            let mut parts = vec!["a".repeat(255); 31];
            parts.push(i.to_string());
            parts
        })
        .collect();
    assert_eq!(validate_file_set(&paths), Err(InputError::TooLarge));
}

#[test]
fn casing_generated_normalization_aliases_collide_including_prefixes() {
    for (left, right) in [("\u{0390}", "\u{03aa}\u{0301}"), ("\u{0130}", "i\u{0307}")] {
        assert_eq!(
            validate_file_set(&files(&[&[left], &[right]])),
            Err(InputError::PathCollision)
        );
        assert_eq!(
            validate_file_set(&files(&[&[left], &[right, "child"]])),
            Err(InputError::PathCollision)
        );
    }
}
