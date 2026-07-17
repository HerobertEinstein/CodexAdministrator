#![cfg(windows)]

use std::time::{SystemTime, UNIX_EPOCH};

use codex_administrator::{CredentialStore, WindowsCredentialStore};

#[test]
fn windows_credential_store_round_trips_updates_and_deletes_the_provider_key() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let target = format!("CodexAdministrator/tests/{}-{suffix}", std::process::id());
    let store = WindowsCredentialStore::new(target);
    let _ = store.delete();

    store.write("test-provider-key-one").unwrap();
    assert_eq!(
        store.read().unwrap().as_deref(),
        Some("test-provider-key-one")
    );

    store.write("test-provider-key-two").unwrap();
    assert_eq!(
        store.read().unwrap().as_deref(),
        Some("test-provider-key-two")
    );

    assert!(store.delete().unwrap());
    assert_eq!(store.read().unwrap(), None);
}
