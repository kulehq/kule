//! The key that seals the values in `secrets.json`. In the person's own
//! Kule home (`~/.kule`, `%USERPROFILE%\.kule` on Windows) the system keeps
//! it: the keychain on macOS, Secret Service on Linux, the Credential
//! Manager on Windows, so the file holds nothing readable on its own, in a
//! backup either. Another home (`KULE_HOME`, a test) or a computer whose
//! system keeps none keeps it in a file beside, readable by its owner alone,
//! and the Secrets view says so.

use std::path::Path;

use base64::Engine;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};

/// Where the key lives when it is not the system's to keep.
pub const KEY_FILE: &str = "secrets.key";
const SERVICE: &str = "Kule secrets";
/// The system's store of keys, as the person knows it.
const STORE: &str = if cfg!(target_os = "macos") {
    "the keychain"
} else if cfg!(windows) {
    "the Credential Manager"
} else {
    "Secret Service"
};

/// Who keeps the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyHome {
    /// The keychain (macOS), Secret Service (Linux) or the Credential
    /// Manager (Windows).
    System,
    /// A file in Kule's home, readable by its owner alone.
    File,
}

pub(crate) struct Sealer {
    key: LessSafeKey,
    pub(crate) home: KeyHome,
}

impl Sealer {
    /// The key for `home`, kept where `kept` says; never kept yet, made
    /// where it belongs (see the module).
    pub(crate) fn open(home: &Path, kept: Option<KeyHome>) -> Result<Sealer> {
        Sealer::open_in(home, kept, persons_home(home))
    }

    /// [`Sealer::open`], the system keeping the key when `system` says so.
    fn open_in(home: &Path, kept: Option<KeyHome>, system: bool) -> Result<Sealer> {
        let account = account(home)?;
        // Windows kept every key in a file up to 0.2.0-preview.15.
        let carry = cfg!(windows) && system;
        let (bytes, where_) = match kept {
            Some(KeyHome::System) => (system_get(&account)?, KeyHome::System),
            Some(KeyHome::File) if carry => carried(home, &account)?,
            Some(KeyHome::File) => (file_get(home)?, KeyHome::File),
            // Made before, by a write that did not get to say so: a file
            // that says which key it was sealed with is not the only one
            // (a task's keeps no record), so a key the system already
            // keeps is used, never replaced.
            None if home.join(KEY_FILE).is_file() && carry => carried(home, &account)?,
            None if home.join(KEY_FILE).is_file() => (file_get(home)?, KeyHome::File),
            None if system && system_has(&account)? => (system_get(&account)?, KeyHome::System),
            None => {
                let fresh = random::<32>()?;
                if system && system_set(&account, &fresh).is_ok() {
                    (fresh.to_vec(), KeyHome::System)
                } else {
                    file_set(home, &fresh)?;
                    (fresh.to_vec(), KeyHome::File)
                }
            }
        };
        let key = UnboundKey::new(&CHACHA20_POLY1305, &bytes)
            .map_err(|_| Error::Invalid("the key to your secrets is not a key".into()))?;
        Ok(Sealer {
            key: LessSafeKey::new(key),
            home: where_,
        })
    }

    /// `value`, sealed to `bound` (what it is for, authenticated with it):
    /// nonce and ciphertext, base64.
    pub(crate) fn seal(&self, value: &str, bound: &[u8]) -> Result<String> {
        let nonce = random::<NONCE_LEN>()?;
        let mut data = value.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(bound),
                &mut data,
            )
            .map_err(|_| Error::Invalid("could not seal a secret".into()))?;
        let mut out = nonce.to_vec();
        out.extend(data);
        Ok(base64::engine::general_purpose::STANDARD.encode(out))
    }

    /// What [`Sealer::seal`] sealed to `bound`; refused when it was changed,
    /// or sealed to anything else.
    pub(crate) fn unseal(&self, sealed: &str, bound: &[u8]) -> Result<String> {
        let bad = || Error::Invalid("a sealed secret does not open with the key".into());
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(sealed)
            .map_err(|_| bad())?;
        if bytes.len() < NONCE_LEN {
            return Err(bad());
        }
        let (nonce, rest) = bytes.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| bad())?;
        let mut data = rest.to_vec();
        let plain = self
            .key
            .open_in_place(nonce, Aad::from(bound), &mut data)
            .map_err(|_| bad())?;
        String::from_utf8(plain.to_vec()).map_err(|_| bad())
    }
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    SystemRandom::new()
        .fill(&mut out)
        .map_err(|_| Error::Invalid("no randomness for a key".into()))?;
    Ok(out)
}

/// The keyring account for `home`: its real path, so two homes keep two
/// keys.
fn account(home: &Path) -> Result<String> {
    std::fs::create_dir_all(home).at(home)?;
    let real = std::fs::canonicalize(home).at(home)?;
    Ok(real.to_string_lossy().into_owned())
}

/// `~/.kule` (`%USERPROFILE%\.kule` on Windows), the person's own home: the
/// one the system keeps a key for.
fn persons_home(home: &Path) -> bool {
    let Some(user) = crate::osfs::person_home() else {
        return false;
    };
    match (
        std::fs::canonicalize(home),
        std::fs::canonicalize(user.join(".kule")),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn system_get(account: &str) -> Result<Vec<u8>> {
    let said = |e: keyring::Error| {
        Error::Invalid(format!(
            "the key to your secrets is in {STORE} and could not be read: {e}"
        ))
    };
    let entry = keyring::Entry::new(SERVICE, account).map_err(said)?;
    let text = entry.get_password().map_err(said)?;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| Error::Invalid(format!("the key in {STORE} is not Kule's")))
}

/// Whether the system keeps a key for `account`; an error where it cannot
/// say (a locked keychain), so a key is never made over one it keeps.
fn system_has(account: &str) -> Result<bool> {
    let entry = keyring::Entry::new(SERVICE, account)
        .map_err(|e| Error::Invalid(format!("{STORE}: {e}")))?;
    match entry.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        // No keychain at all (a Linux without Secret Service): none kept.
        Err(keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_)) => Ok(false),
        Err(e) => Err(Error::Invalid(format!(
            "the key to your secrets may be in {STORE} and could not be read: {e}"
        ))),
    }
}

fn system_set(account: &str, key: &[u8]) -> std::result::Result<(), keyring::Error> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    entry.set_password(&base64::engine::general_purpose::STANDARD.encode(key))?;
    // Read back: a keyring that takes it but cannot give it back is none.
    entry.get_password().map(|_| ())
}

/// Windows: the key in `secrets.key` moved to the Credential Manager and the
/// file removed, so a home from before keeps opening its secrets and the
/// file no longer holds them. The file stays where the system takes none,
/// or already keeps another key for the home (never replaced); with the
/// file gone, the key is read from the system (moved before: a task's
/// secrets still say "file").
fn carried(home: &Path, account: &str) -> Result<(Vec<u8>, KeyHome)> {
    let path = home.join(KEY_FILE);
    if !path.is_file() {
        return Ok((system_get(account)?, KeyHome::System));
    }
    let key = file_get(home)?;
    let held = match system_has(account) {
        Ok(true) => system_get(account).ok(),
        Ok(false) => system_set(account, &key)
            .ok()
            .and_then(|()| system_get(account).ok()),
        Err(_) => None,
    };
    // Said where it is: in the file still, until the file is gone.
    if held.as_deref() != Some(&key[..]) || std::fs::remove_file(&path).is_err() {
        return Ok((key, KeyHome::File));
    }
    Ok((key, KeyHome::System))
}

fn file_get(home: &Path) -> Result<Vec<u8>> {
    let path = home.join(KEY_FILE);
    let text = std::fs::read_to_string(&path).at(&path)?;
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| Error::Invalid(format!("{} is not a key", path.display())))
}

fn file_set(home: &Path, key: &[u8]) -> Result<()> {
    use std::io::Write;
    let path = home.join(KEY_FILE);
    crate::osfs::create_private(&path)
        .and_then(|mut f| {
            f.write_all(
                base64::engine::general_purpose::STANDARD
                    .encode(key)
                    .as_bytes(),
            )
        })
        .at(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system's keychain keeps a key and gives it back; skipped where
    /// there is none to reach (a CI machine's may be locked).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_keeps_a_key_and_gives_it_back() {
        let account = format!(
            "kule-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let key = [7u8; 32];
        if let Err(e) = system_set(&account, &key) {
            eprintln!("skipped: no keychain to reach here: {e}");
            return;
        }
        let back = system_get(&account);
        let _ = keyring::Entry::new(SERVICE, &account).and_then(|e| e.delete_credential());
        assert_eq!(back.unwrap(), key);
    }

    /// A system entry for a home of the test's own, forgotten at the end.
    #[cfg(windows)]
    struct Kept(String);

    #[cfg(windows)]
    impl Drop for Kept {
        fn drop(&mut self) {
            let _ = keyring::Entry::new(SERVICE, &self.0).and_then(|e| e.delete_credential());
        }
    }

    /// Whether the Credential Manager keeps a key here: Windows gives an
    /// SSH session signed in with a key none. Says why not, and fails where
    /// the store is required (`KULE_TEST_REQUIRE_SANDBOX`: the Windows test
    /// machine and CI).
    #[cfg(windows)]
    fn store_here(account: &str) -> bool {
        let Err(e) = system_set(account, &[1u8; 32]) else {
            let _ = keyring::Entry::new(SERVICE, account).and_then(|e| e.delete_credential());
            return true;
        };
        assert!(
            std::env::var_os("KULE_TEST_REQUIRE_SANDBOX").is_none(),
            "the Credential Manager is required here, but: {e}"
        );
        eprintln!("skipped: the Credential Manager keeps nothing here: {e}");
        false
    }

    /// On Windows the person's own home is the one in their profile,
    /// whatever `HOME` says (a shell of Git's sets it, Windows does not).
    #[cfg(windows)]
    #[test]
    fn the_persons_own_home_is_the_one_in_the_profile() {
        let home = crate::osfs::person_home().unwrap().join(".kule");
        let made = !home.exists();
        std::fs::create_dir_all(&home).unwrap();
        let mine = persons_home(&home);
        if made {
            let _ = std::fs::remove_dir(&home);
        }
        assert!(mine, "{} is not the person's own home", home.display());
    }

    /// A key a file kept (every Windows home up to 0.2.0-preview.15) moves
    /// to the Credential Manager, the file goes, and what it sealed still
    /// opens, from the home's secrets and a task's that still say "file".
    #[cfg(windows)]
    #[test]
    fn a_key_kept_in_a_file_moves_to_the_credential_manager() {
        let home = tempfile::TempDir::new().unwrap();
        if !store_here(&account(home.path()).unwrap()) {
            return;
        }
        let before = Sealer::open_in(home.path(), None, false).unwrap();
        assert_eq!(before.home, KeyHome::File);
        let sealed = before.seal("sk-real", b"for").unwrap();
        let _kept = Kept(account(home.path()).unwrap());

        let moved = Sealer::open_in(home.path(), Some(KeyHome::File), true).unwrap();
        assert_eq!(moved.home, KeyHome::System);
        assert!(
            !home.path().join(KEY_FILE).exists(),
            "the file still holds the key"
        );
        assert_eq!(moved.unseal(&sealed, b"for").unwrap(), "sk-real");

        let task = Sealer::open_in(home.path(), Some(KeyHome::File), true).unwrap();
        assert_eq!(task.home, KeyHome::System);
        assert_eq!(task.unseal(&sealed, b"for").unwrap(), "sk-real");
    }

    /// A key the Credential Manager already keeps for the home is never
    /// replaced by the file's: the file stays, and says so.
    #[cfg(windows)]
    #[test]
    fn a_key_the_system_keeps_already_is_never_replaced() {
        let home = tempfile::TempDir::new().unwrap();
        let account = account(home.path()).unwrap();
        if !store_here(&account) {
            return;
        }
        let before = Sealer::open_in(home.path(), None, false).unwrap();
        let sealed = before.seal("sk-real", b"for").unwrap();
        let _kept = Kept(account.clone());
        let other = [9u8; 32];
        system_set(&account, &other).unwrap();

        let after = Sealer::open_in(home.path(), Some(KeyHome::File), true).unwrap();
        assert_eq!(after.home, KeyHome::File);
        assert!(home.path().join(KEY_FILE).is_file());
        assert_eq!(after.unseal(&sealed, b"for").unwrap(), "sk-real");
        assert_eq!(system_get(&account).unwrap(), other);
    }

    #[test]
    fn a_value_sealed_opens_only_with_its_key_and_unchanged() {
        let home = tempfile::TempDir::new().unwrap();
        let sealer = Sealer::open(home.path(), None).unwrap();
        // A home other than the person's keeps its key in a file.
        assert_eq!(sealer.home, KeyHome::File);
        let sealed = sealer.seal("sk-real", b"for").unwrap();
        assert!(!sealed.contains("sk-real"));
        assert_eq!(sealer.unseal(&sealed, b"for").unwrap(), "sk-real");
        // Opened only for what it was sealed to.
        assert!(sealer.unseal(&sealed, b"else").is_err());
        assert!(sealer.unseal(&sealed, b"").is_err());
        // Twice sealed, twice different.
        assert_ne!(sealer.seal("sk-real", b"for").unwrap(), sealed);

        let again = Sealer::open(home.path(), Some(KeyHome::File)).unwrap();
        assert_eq!(again.unseal(&sealed, b"for").unwrap(), "sk-real");
        let mut changed = base64::engine::general_purpose::STANDARD
            .decode(&sealed)
            .unwrap();
        *changed.last_mut().unwrap() ^= 1;
        let changed = base64::engine::general_purpose::STANDARD.encode(changed);
        assert!(again.unseal(&changed, b"for").is_err());

        let other = tempfile::TempDir::new().unwrap();
        let stranger = Sealer::open(other.path(), None).unwrap();
        assert!(stranger.unseal(&sealed, b"for").is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.path().join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "{mode:o}");
        }
    }
}
