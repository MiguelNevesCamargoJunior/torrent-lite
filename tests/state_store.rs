use torrent_lite::state_store::{AddInput, DesiredState, SourceKind, StateStore, StoreError};

fn magnet(n: u64) -> String {
    format!("magnet:?xt=urn:btih:{n:040x}&dn=sample")
}

#[test]
fn registration_is_idempotent_and_conflicts_on_a_different_destination() {
    let workspace = tempfile::tempdir().unwrap();
    let root_a = workspace.path().join("a");
    let root_b = workspace.path().join("b");
    std::fs::create_dir(&root_a).unwrap();
    std::fs::create_dir(&root_b).unwrap();
    let mut store = StateStore::open(&workspace.path().join("state.db")).unwrap();
    let source = magnet(1);
    let first = store
        .register(AddInput::Magnet(&source), &root_a, true)
        .unwrap();
    assert_eq!(first.desired, DesiredState::Paused);
    assert_eq!(first.source_kind, SourceKind::Magnet);
    let repeated = store
        .register(AddInput::Magnet(&source), &root_a, false)
        .unwrap();
    assert_eq!(first.id, repeated.id);
    assert_eq!(repeated.desired, DesiredState::Paused);
    assert!(matches!(
        store.register(AddInput::Magnet(&source), &root_b, false),
        Err(StoreError::Conflict)
    ));
    assert_eq!(store.recover().unwrap().len(), 1);
}

#[test]
fn intent_and_tombstone_survive_reopen_without_touching_downloads() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("downloads");
    std::fs::create_dir(&root).unwrap();
    let data = root.join("keep.bin");
    std::fs::write(&data, b"untouched").unwrap();
    let database = workspace.path().join("state.db");
    let source = magnet(2);
    let id = {
        let mut store = StateStore::open(&database).unwrap();
        let record = store
            .register(AddInput::Magnet(&source), &root, false)
            .unwrap();
        store.set_intent(&record.id, DesiredState::Paused).unwrap();
        store.set_intent(&record.id, DesiredState::Paused).unwrap();
        record.id
    };
    {
        let mut store = StateStore::open(&database).unwrap();
        let active = store.recover().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id);
        assert_eq!(active[0].desired, DesiredState::Paused);
        store.set_intent(&id, DesiredState::Running).unwrap();
        store.remove(&id).unwrap();
        store.remove(&id).unwrap();
        assert!(store.recover().unwrap().is_empty());
        assert!(matches!(
            store.register(AddInput::Magnet(&source), &root, false),
            Err(StoreError::Conflict)
        ));
    }
    assert_eq!(std::fs::read(&data).unwrap(), b"untouched");
    assert!(
        StateStore::open(&database)
            .unwrap()
            .recover()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unknown_future_schema_fails_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let database = workspace.path().join("future.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    assert!(matches!(
        StateStore::open(&database),
        Err(StoreError::UnsupportedSchema)
    ));
    let connection = rusqlite::Connection::open(&database).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        99
    );
}

#[test]
fn declared_version_without_expected_table_fails_closed() {
    let workspace = tempfile::tempdir().unwrap();
    let database = workspace.path().join("inconsistent.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute_batch("PRAGMA application_id=1953261940; PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    assert!(matches!(
        StateStore::open(&database),
        Err(StoreError::UnsupportedSchema)
    ));
}

#[test]
fn one_process_owns_the_state_database_at_a_time() {
    let workspace = tempfile::tempdir().unwrap();
    let database = workspace.path().join("state.db");
    let first = StateStore::open(&database).unwrap();
    assert!(matches!(StateStore::open(&database), Err(StoreError::Busy)));
    drop(first);
    let second = StateStore::open(&database).unwrap();
    assert!(second.recover().unwrap().is_empty());
}

#[test]
fn second_process_cannot_open_the_owned_database() {
    let workspace = tempfile::tempdir().unwrap();
    let database = workspace.path().join("state.db");
    let first = StateStore::open(&database).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_child"])
        .env("TORRENT_LITE_LOCK_CHILD", &database)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    drop(first);
    assert!(StateStore::open(&database).is_ok());
}

#[test]
fn lock_child() {
    if let Some(database) = std::env::var_os("TORRENT_LITE_LOCK_CHILD") {
        assert!(matches!(
            StateStore::open(&std::path::PathBuf::from(database)),
            Err(StoreError::Busy)
        ));
    }
}

#[test]
fn metadata_and_registration_count_are_bounded() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("downloads");
    std::fs::create_dir(&root).unwrap();
    let mut store = StateStore::open(&workspace.path().join("state.db")).unwrap();
    let oversized = "x".repeat(16 * 1024 + 1);
    assert!(matches!(
        store.register(AddInput::Magnet(&oversized), &root, false),
        Err(StoreError::Input(_))
    ));
    for i in 0..100 {
        store
            .register(AddInput::Magnet(&magnet(i)), &root, false)
            .unwrap();
    }
    assert!(matches!(
        store.register(AddInput::Magnet(&magnet(100)), &root, false),
        Err(StoreError::Limit)
    ));
    assert_eq!(store.recover().unwrap().len(), 100);
}

#[test]
fn v1_torrent_bytes_are_registered_and_restored() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().join("downloads");
    std::fs::create_dir(&root).unwrap();
    let mut bytes = b"d4:infod6:lengthi4e4:name8:file.txt12:piece lengthi4e6:pieces20:".to_vec();
    bytes.extend([0_u8; 20]);
    bytes.extend(b"ee");
    let database = workspace.path().join("state.db");
    let record = StateStore::open(&database)
        .unwrap()
        .register(AddInput::Torrent(&bytes), &root, true)
        .unwrap();
    assert_eq!(record.source_kind, SourceKind::Torrent);
    let reopened = StateStore::open(&database).unwrap();
    assert_eq!(reopened.load_source(&record.id).unwrap(), bytes);
}
