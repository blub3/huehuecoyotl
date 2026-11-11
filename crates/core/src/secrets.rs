//! Credentials in the OS keystore (SPEC 6): Windows Credential Manager, the Secret Service on
//! Linux (the secrets portal under Flatpak), Keychain on macOS. The database never sees them.
//! `COYOTL_TMDB_KEY` overrides the keystore for development and automated tests; it is never
//! written anywhere. `COYOTL_DISABLE_KEYSTORE` turns the keystore off (tests).

const SERVICE: &str = "coyotl";
const TMDB_USER: &str = "tmdb-api-key";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    Keystore,
}

/// Blocking: call from `spawn_blocking`.
pub fn load_tmdb_key() -> Option<(String, KeySource)> {
    // Tests and sandboxed runs must never pick up the user's real key.
    if std::env::var_os("COYOTL_DISABLE_KEYSTORE").is_some() {
        return None;
    }
    if let Ok(k) = std::env::var("COYOTL_TMDB_KEY") {
        if !k.trim().is_empty() {
            return Some((k.trim().to_string(), KeySource::Environment));
        }
    }
    let entry = keyring::Entry::new(SERVICE, TMDB_USER).ok()?;
    match entry.get_password() {
        Ok(k) if !k.trim().is_empty() => Some((k, KeySource::Keystore)),
        Ok(_) => None,
        Err(e) => {
            tracing::debug!("no TMDB key in the keystore: {e}");
            None
        }
    }
}

/// Blocking. Returns a user-facing reason when the keystore is unavailable.
pub fn store_tmdb_key(key: &str) -> Result<(), String> {
    keyring::Entry::new(SERVICE, TMDB_USER)
        .and_then(|e| e.set_password(key))
        .map_err(|e| {
            format!(
                "The system keystore is not available ({e}). The key works until the app closes."
            )
        })
}

/// Blocking.
pub fn clear_tmdb_key() {
    if let Ok(e) = keyring::Entry::new(SERVICE, TMDB_USER) {
        let _ = e.delete_credential();
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
