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
