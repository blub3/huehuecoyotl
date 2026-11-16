//! Credentials in the OS keystore (SPEC 6): Windows Credential Manager, the Secret Service on
//! Linux (also under Flatpak, which may talk to `org.freedesktop.secrets`), Keychain on macOS.
//! The database never sees them, and they are never logged. `COYOTL_TMDB_KEY` overrides the
//! keystore for development and automated tests; it is never written anywhere.
//! `COYOTL_DISABLE_KEYSTORE` turns the keystore off (tests).

const SERVICE: &str = "coyotl";
const TMDB_USER: &str = "tmdb-api-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    Keystore,
    /// Issue #52: entered, but the keystore could not keep it; it works until the app closes.
    Session,
}

/// What the start-up found for the TMDB key (issue #52).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyLoad {
    Found(String, KeySource),
    /// The keystore works and has no key.
    Absent,
    /// No keystore can be reached (none installed, locked, or it keeps nothing): the reason.
    Unavailable(String),
}

/// The key state shown in Settings (issue #52): `keystore`, `environment`, `session` (could not
/// be stored), `none`, or `unavailable` (no key, and no keystore to keep one).
pub fn key_state(source: Option<KeySource>, keystore: &Result<(), String>) -> &'static str {
    match source {
        Some(KeySource::Keystore) => "keystore",
        Some(KeySource::Environment) => "environment",
        Some(KeySource::Session) => "session",
        None if keystore.is_err() => "unavailable",
        None => "none",
    }
}

/// Blocking. Whether a keystore that keeps secrets across restarts is available. A store that
/// keeps them in memory only (keyring's mock store) counts as unavailable, so a secret is never
/// silently lost at exit.
pub fn keystore_status() -> Result<(), String> {
    if let Err(e) = keyring::Entry::store_status() {
        return Err(e.to_string());
    }
    match keyring_core::get_default_store().map(|s| s.persistence()) {
        Some(keyring_core::CredentialPersistence::EntryOnly)
        | Some(keyring_core::CredentialPersistence::ProcessOnly) => {
            Err("the credential store keeps nothing after the app closes".into())
        }
        Some(_) => Ok(()),
        None => Err("no credential store".into()),
    }
}

/// Blocking: call from `spawn_blocking`.
pub fn load_tmdb_key() -> KeyLoad {
    // Tests and sandboxed runs must never pick up the user's real key.
    if std::env::var_os("COYOTL_DISABLE_KEYSTORE").is_some() {
        return KeyLoad::Absent;
    }
    if let Ok(k) = std::env::var("COYOTL_TMDB_KEY") {
        if !k.trim().is_empty() {
            return KeyLoad::Found(k.trim().to_string(), KeySource::Environment);
        }
    }
    if let Err(e) = keystore_status() {
        return KeyLoad::Unavailable(e);
    }
    let entry = match keyring::Entry::new(SERVICE, TMDB_USER) {
        Ok(e) => e,
        Err(e) => return KeyLoad::Unavailable(e.to_string()),
    };
    match entry.get_password() {
        Ok(k) if !k.trim().is_empty() => KeyLoad::Found(k, KeySource::Keystore),
        Ok(_) | Err(keyring::Error::NoEntry) => KeyLoad::Absent,
        Err(e @ (keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_))) => {
            tracing::warn!("the system keystore cannot be read: {e}");
            KeyLoad::Unavailable(e.to_string())
        }
        Err(e) => {
            tracing::warn!("the stored TMDB key cannot be read: {e}");
            KeyLoad::Absent
        }
    }
}

/// Blocking. Returns a user-facing reason when the keystore is unavailable.
/// Never falls back to another place: without a keystore the key is not kept at all.
pub fn store_tmdb_key(key: &str) -> Result<(), String> {
    let unavailable = |e: String| {
        format!(
            "The key could not be stored: the system keystore is not available ({e}). It works until the app closes. Install or unlock a keyring (for example GNOME Keyring or KWallet), then enter the key again."
        )
    };
    keystore_status().map_err(unavailable)?;
    keyring::Entry::new(SERVICE, TMDB_USER)
        .and_then(|e| e.set_password(key))
        .map_err(|e| unavailable(e.to_string()))
}

/// Blocking. Removes the keystore entry; an entry that is already gone is fine.
pub fn clear_tmdb_key() -> Result<(), String> {
    if std::env::var_os("COYOTL_DISABLE_KEYSTORE").is_some() {
        return Ok(());
    }
    match keyring::Entry::new(SERVICE, TMDB_USER).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!(
            "The key could not be removed from the system keystore ({e})."
        )),
    }
}

// ---- network source passwords --------------------------------------------------------------

/// Passwords kept for this session when the keystore is off or unavailable.
fn memory() -> &'static std::sync::Mutex<std::collections::HashMap<String, String>> {
    static M: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

fn keystore_disabled() -> bool {
    std::env::var_os("COYOTL_DISABLE_KEYSTORE").is_some()
}

/// Blocking. `reference` is the source's `credential_ref`. Returns a user-facing warning when
/// the password can only be kept until the app closes.
pub fn store_password(reference: &str, password: &str) -> Result<(), String> {
    memory()
        .lock()
        .unwrap()
        .insert(reference.to_string(), password.to_string());
    if keystore_disabled() {
        return Ok(());
    }
    if password.is_empty() {
        delete_password(reference);
        return Ok(());
    }
    keyring::Entry::new(SERVICE, reference)
        .and_then(|e| e.set_password(password))
        .map_err(|e| {
            format!(
                "The system keystore is not available ({e}). The password works until the app closes."
            )
        })
}

/// Blocking. An empty string when nothing is stored (guest access, NFS).
pub fn load_password(reference: &str) -> String {
    if let Some(p) = memory().lock().unwrap().get(reference) {
        return p.clone();
    }
    if keystore_disabled() {
        return String::new();
    }
    let p = keyring::Entry::new(SERVICE, reference)
        .and_then(|e| e.get_password())
        .unwrap_or_default();
    memory()
        .lock()
        .unwrap()
        .insert(reference.to_string(), p.clone());
    p
}

/// Blocking.
pub fn delete_password(reference: &str) {
    memory().lock().unwrap().remove(reference);
    if keystore_disabled() {
        return;
    }
    if let Ok(e) = keyring::Entry::new(SERVICE, reference) {
        let _ = e.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_state() {
        let ok = Ok(());
        let gone = Err("no keystore".to_string());
        assert_eq!(key_state(Some(KeySource::Keystore), &ok), "keystore");
        assert_eq!(key_state(Some(KeySource::Environment), &ok), "environment");
        assert_eq!(
            key_state(Some(KeySource::Environment), &gone),
            "environment"
        );
        assert_eq!(key_state(Some(KeySource::Session), &gone), "session");
        assert_eq!(key_state(None, &ok), "none");
        assert_eq!(key_state(None, &gone), "unavailable");
    }

    /// Release builds must keep secrets in a real keystore: keyring's in-memory mock store would
    /// lose them at exit. Where a store is available, it must persist.
    #[test]
    fn the_keystore_is_never_the_mock_store() {
        // Only inside the throwaway keyring of tools/test-keystore.sh, never the user's.
        if std::env::var_os("COYOTL_TEST_KEYRING").is_none() {
            return;
        }
        {
            assert!(
                keyring::Entry::store_status().is_ok(),
                "no keystore in the test session"
            );
            let store = keyring_core::get_default_store().expect("a default store");
            assert!(!store.vendor().contains("Mock"), "{}", store.vendor());
            assert!(!matches!(
                store.persistence(),
                keyring_core::CredentialPersistence::EntryOnly
                    | keyring_core::CredentialPersistence::ProcessOnly
            ));
        }
    }
}
