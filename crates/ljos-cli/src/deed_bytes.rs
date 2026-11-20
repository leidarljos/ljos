//! The bytes of a stored deed, checked before anyone reads them.
//!
//! `deedar` has no verb that prints a deed's bytes, and they sit in the
//! store as `bytes/<sha256>`. [`read_deed`] asks `deedar get` for the
//! digest, runs `deedar evidence`, and hashes the blob it read before
//! handing it on, so a missing or altered blob is a refusal.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

/// A deed's bytes, read back and checked.
pub struct DeedBytes {
    pub accession: String,
    pub name: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

pub(crate) fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    text.lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
}

/// The `name=` of the deed `deedar get` printed. The first line is
/// `id=... kind=... name=NAME`, and the name may hold spaces.
pub(crate) fn deed_name(text: &str) -> String {
    text.lines()
        .next()
        .and_then(|line| line.split_once(" name="))
        .map(|(_, name)| name.trim().to_string())
        .unwrap_or_default()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The bytes of one stored deed, after `deedar evidence` passes and the
/// blob in the store hashes to the digest the deed names.
///
/// # Errors
///
/// The deed store does not hold the accession, the deed names no bytes or
/// more than one file, `deedar evidence` fails, the blob is missing or a
/// symlink, or it hashes to another digest.
pub fn read_deed(accession: &str) -> Result<DeedBytes> {
    let token = crate::deed_token(accession);
    if token.is_empty() {
        bail!("deed: an accession is required");
    }
    let got = crate::run_captured("deedar", &["get", &token])
        .map_err(|e| anyhow::anyhow!("deed: {token} is not in the deed store: {e:#}"))?;
    let id = field(&got.stdout, "id")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or(&token)
        .to_string();
    let name = deed_name(&got.stdout);
    let paths: Vec<&str> = field(&got.stdout, "paths")
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let hex = match paths.as_slice() {
        [] => bail!("deed: {id} stores no bytes; `deedar get {id}` prints what it records"),
        [one] => one
            .strip_prefix("sha256:")
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
            .with_context(|| format!("deed: {id} names {one}, which is not a sha256 digest"))?
            .to_ascii_lowercase(),
        many => bail!(
            "deed: {id} names {} files; this reads a deed of one file",
            many.len()
        ),
    };
    if let Err(e) = crate::run_captured("deedar", &["evidence", &id]) {
        bail!("deed: {id} failed `deedar evidence`, so nothing was printed: {e:#}");
    }
    let blob = crate::deedbag::store_dir()?.join("bytes").join(&hex);
    match std::fs::symlink_metadata(&blob) {
        Ok(meta) if meta.file_type().is_symlink() => {
            bail!("deed: the bytes of {id} at {} are a symlink; nothing was printed", blob.display())
        }
        Ok(meta) if meta.is_file() => {}
        Ok(_) | Err(_) => bail!(
            "deed: the bytes of {id} (sha256:{hex}) are missing from the deed store at {}; nothing was printed",
            blob.display()
        ),
    }
    let bytes =
        std::fs::read(&blob).with_context(|| format!("deed: reading {}", blob.display()))?;
    let found = sha256_hex(&bytes);
    if found != hex {
        bail!(
            "deed: the bytes of {id} are corrupt: {} hashes to sha256:{found}, the deed names sha256:{hex}; nothing was printed",
            blob.display()
        );
    }
    Ok(DeedBytes {
        accession: id,
        name,
        sha256: hex,
        bytes,
    })
}
