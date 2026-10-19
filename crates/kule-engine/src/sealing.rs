//! The key that seals the values in `secrets.json`. In the person's own
//! Kule home (`~/.kule`) the system keeps it: the keychain on macOS, Secret
//! Service on Linux, so the file holds nothing readable on its own, in a
//! backup either. Another home (`KULE_HOME`, a test) or a computer without a
//! keyring keeps it in a file beside, readable by its owner alone, and the
//! Secrets view says so.

use std::path::{Path, PathBuf};

use base64::Engine;
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};

/// Where the key lives when it is not the system's to keep.
pub const KEY_FILE: &str = "secrets.key";
const SERVICE: &str = "Kule secrets";

/// Who keeps the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyHome {
    /// The keychain (macOS) or Secret Service (Linux).
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
        let account = account(home)?;
        let (bytes, where_) = match kept {
            Some(KeyHome::System) => (system_get(&account)?, KeyHome::System),
            Some(KeyHome::File) => (file_get(home)?, KeyHome::File),
            // Made before, by a write that did not get to say so: a file
            // that says which key it was sealed with is not the only one
            // (a task's keeps no record), so a key the system already
            // keeps is used, never replaced.
            None if home.join(KEY_FILE).is_file() => (file_get(home)?, KeyHome::File),
            None if persons_home(home) && system_has(&account)? => {
                (system_get(&account)?, KeyHome::System)
            }
            None => {
                let fresh = random::<32>()?;
                if persons_home(home) && system_set(&account, &fresh).is_ok() {
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

/// `~/.kule`, the person's own home: the one the system keeps a key for.
fn persons_home(home: &Path) -> bool {
    let Some(user) = std::env::var_os("HOME").map(PathBuf::from) else {
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
            "the key to your secrets is in the system's keychain and could not be read: {e}"
        ))
    };
    let entry = keyring::Entry::new(SERVICE, account).map_err(said)?;
    let text = entry.get_password().map_err(said)?;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|_| Error::Invalid("the key in the keychain is not Kule's".into()))
}

/// Whether the system keeps a key for `account`; an error where it cannot
/// say (a locked keychain), so a key is never made over one it keeps.
fn system_has(account: &str) -> Result<bool> {
    let entry = keyring::Entry::new(SERVICE, account)
        .map_err(|e| Error::Invalid(format!("the system's keychain: {e}")))?;
    match entry.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        // No keychain at all (a Linux without Secret Service): none kept.
        Err(keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_)) => Ok(false),
        Err(e) => Err(Error::Invalid(format!(
            "the key to your secrets may be in the system's keychain and could not be read: {e}"
        ))),
    }
}

fn system_set(account: &str, key: &[u8]) -> std::result::Result<(), keyring::Error> {
    let entry = keyring::Entry::new(SERVICE, account)?;
    entry.set_password(&base64::engine::general_purpose::STANDARD.encode(key))?;
    // Read back: a keyring that takes it but cannot give it back is none.
    entry.get_password().map(|_| ())
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
    use std::os::unix::fs::OpenOptionsExt;
    let path = home.join(KEY_FILE);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
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
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.path().join(KEY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
}
