//! The person's passphrase. A shell can carry a runner's variable on a
//! host that exports it to everything it starts, and then the gates
//! cannot tell the person from a runner by the process alone. The
//! passphrase is what the person knows and a runner does not.
//!
//! Only an argon2id hash is kept, in PHC form with a random salt, in
//! `person-key` under the ljos config directory of the home the passwd
//! entry names. `HOME` and `XDG_CONFIG_HOME` do not move it: a runner
//! sets both for what it starts.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use argon2::password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString};
use argon2::Argon2;

/// The shortest passphrase `ljos person-key set` takes, in bytes.
pub const MIN_LEN: usize = 12;

const KEY_NAME: &str = "person-key";

/// The ljos config directory of the passwd entry's home.
#[must_use]
pub fn key_dir() -> Option<PathBuf> {
    crate::proc_tree::own_home().map(|home| home.join(".config").join("ljos"))
}

/// Where the hash is kept.
#[must_use]
pub fn key_path() -> Option<PathBuf> {
    key_dir().map(|dir| dir.join(KEY_NAME))
}

/// The PHC string of `pass` under a fresh salt from the OS.
///
/// # Errors
///
/// The OS gave no random bytes, or the hash failed.
pub fn hash(pass: &[u8]) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| anyhow::anyhow!("person-key: no salt from the OS: {e}"))?;
    let salt = SaltString::encode_b64(&bytes).map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    Argon2::default()
        .hash_password(pass, &salt)
        .map(|phc| phc.to_string())
        .map_err(|e| anyhow::anyhow!("person-key: {e}"))
}

/// `phc` read as an argon2id hash with its salt and output. Any other
/// algorithm is not a key.
///
/// # Errors
///
/// The text is not a PHC string, names another algorithm, or lacks a
/// salt or an output.
pub fn parse(phc: &str) -> Result<PasswordHash<'_>> {
    let parsed = PasswordHash::new(phc.trim()).map_err(|e| anyhow::anyhow!("person-key: {e}"))?;
    if parsed.algorithm != argon2::Algorithm::Argon2id.ident() {
        bail!("person-key: {} is not argon2id", parsed.algorithm);
    }
    if parsed.salt.is_none() || parsed.hash.is_none() {
        bail!("person-key: the PHC string has no salt or no hash");
    }
    Ok(parsed)
}

/// Whether `pass` is the passphrase `phc` was made from. The comparison
/// is the hash crate's, in constant time. A hash that does not parse
/// matches nothing.
#[must_use]
pub fn verify(pass: &[u8], phc: &str) -> bool {
    parse(phc).is_ok_and(|parsed| Argon2::default().verify_password(pass, &parsed).is_ok())
}

/// Write `phc` to `path`, mode 0600. The text goes to a new file beside
/// it that is renamed over the old one, so a reader sees either.
///
/// # Errors
///
/// The directory or the file cannot be written.
pub fn store(path: &Path, phc: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let dir = path
        .parent()
        .context("person-key: the key path has no directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("person-key: {}", dir.display()))?;
    let tmp = dir.join(format!(".{KEY_NAME}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(phc.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        std::fs::File::open(dir)?.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("person-key: {}", path.display()));
    }
    Ok(())
}

/// The hash kept at `path`. `Ok(None)` is no key. A file that is not a
/// regular file, or not argon2id, is an error.
///
/// # Errors
///
/// The file cannot be read or does not hold a key.
pub fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("person-key: {}", path.display())),
        Ok(meta) if !meta.is_file() => {
            bail!("person-key: {} is not a regular file", path.display())
        }
        Ok(_) => {}
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("person-key: {}", path.display()))?;
    parse(&text).with_context(|| format!("person-key: {}", path.display()))?;
    Ok(Some(text.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_matches_its_passphrase_only() {
        let phc = hash(b"correct horse battery").unwrap();
        assert!(phc.starts_with("$argon2id$"), "{phc}");
        assert!(verify(b"correct horse battery", &phc));
        assert!(!verify(b"correct horse batterz", &phc));
        assert!(!verify(b"", &phc));
        let again = hash(b"correct horse battery").unwrap();
        assert_ne!(phc, again, "two hashes shared a salt");
    }

    #[test]
    fn only_an_argon2id_phc_string_parses() {
        let phc = hash(b"correct horse battery").unwrap();
        let parsed = parse(&phc).unwrap();
        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert!(parsed.salt.is_some());
        let argon2i = phc.replacen("$argon2id$", "$argon2i$", 1);
        assert!(parse(&argon2i).is_err());
        assert!(!verify(b"correct horse battery", &argon2i));
        for junk in [
            "",
            "correct horse battery",
            "$argon2id$",
            "$argon2id$v=19$m=1",
        ] {
            assert!(parse(junk).is_err(), "{junk:?}");
            assert!(!verify(b"correct horse battery", junk));
        }
    }

    #[test]
    fn the_stored_key_is_mode_0600_and_reads_back() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ljos").join(KEY_NAME);
        assert!(read(&path).unwrap().is_none());
        let phc = hash(b"correct horse battery").unwrap();
        store(&path, &phc).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(read(&path).unwrap().as_deref(), Some(phc.as_str()));
        let next = hash(b"another long phrase").unwrap();
        store(&path, &next).unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some(next.as_str()));
        let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from(KEY_NAME)]);
        std::fs::write(&path, "not a key\n").unwrap();
        assert!(read(&path).is_err());
    }

    #[test]
    fn a_link_in_place_of_the_key_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("elsewhere");
        store(&real, &hash(b"correct horse battery").unwrap()).unwrap();
        let path = dir.path().join(KEY_NAME);
        std::os::unix::fs::symlink(&real, &path).unwrap();
        assert!(read(&path).is_err());
    }

    /// `HOME` and `XDG_CONFIG_HOME` point at a temporary directory, and
    /// the key path still names the passwd entry's home.
    #[test]
    fn the_key_path_follows_the_passwd_home() {
        let _g = crate::tests::env_guard();
        let own = crate::proc_tree::own_home().expect("a passwd home");
        let tmp = tempfile::tempdir().unwrap();
        let saved: Vec<_> = ["HOME", "XDG_CONFIG_HOME"]
            .iter()
            .map(|k| (*k, std::env::var_os(k)))
            .collect();
        // SAFETY: under env_guard, and restored below.
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("cfg"));
        }
        let got = key_path();
        unsafe {
            for (k, v) in saved {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
        assert_eq!(got, Some(own.join(".config/ljos/person-key")));
        assert!(!got.unwrap().starts_with(tmp.path()));
    }
}
