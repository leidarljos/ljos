//! The optional `[mail]` table.
//!
//! It names the identity drafts are sent as, the deny list, and the
//! notmuch database, so only the user may set it. It is read from the
//! user's own files, `ljos.toml` then `mail.toml` in the ljos config dir,
//! each owned by the user and not group or world writable. Any other
//! config layer (`./ljos.toml`, `./vissue.toml`, `VISSUE_CONFIG`,
//! `LJOS_CONFIG`, the vissue config) that carries a `[mail]` table is an
//! error, as is a layer that cannot be read or parsed: a seat that can
//! write the working directory must not swap the identity or the deny
//! list.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;

/// What the `[mail]` layers say. Every key is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailConfig {
    /// `NOTMUCH_CONFIG` for every read. The reader refuses without it.
    pub notmuch_config: Option<PathBuf>,
    /// The notmuch binary: an absolute path, or a bare name found on PATH.
    pub notmuch: Option<PathBuf>,
    /// The one address a draft's From may carry. Needs `deny_file`.
    pub identity: Option<String>,
    /// The deny list the content checks read.
    pub deny_file: Option<PathBuf>,
    /// Attribution phrases refused on top of the built-in ones.
    pub attribution: Vec<String>,
}

#[derive(Deserialize)]
struct Layer {
    mail: Option<Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    notmuch_config: Option<PathBuf>,
    notmuch: Option<PathBuf>,
    identity: Option<String>,
    deny_file: Option<PathBuf>,
    attribution: Option<Vec<String>>,
}

/// The `[mail]` config from the user's files, after checking that no
/// other layer sets it. `None` when no user file has the table.
pub fn load() -> Result<Option<MailConfig>> {
    let user: Vec<PathBuf> = crate::converge::config_base()
        .map(|base| vec![base.join("ljos/ljos.toml"), base.join("ljos/mail.toml")])
        .unwrap_or_default();
    let shared: Vec<PathBuf> = crate::converge::config_paths(None)
        .into_iter()
        .filter(|path| !user.contains(path))
        .collect();
    load_from(&user, &shared)
}

/// The `[mail]` config from the `user` files, in order, a later one
/// overriding the keys it sets. A `shared` file with a `[mail]` table is
/// an error. A missing file is skipped; any other read failure is an
/// error.
pub fn load_from(user: &[PathBuf], shared: &[PathBuf]) -> Result<Option<MailConfig>> {
    for path in shared {
        let Some(text) = read(path)? else {
            continue;
        };
        let layer: toml::Value =
            toml::from_str(&text).with_context(|| format!("mail config {}", path.display()))?;
        if layer.get("mail").is_some() {
            bail!(
                "mail config {}: [mail] may be set only in the user's own ljos.toml or mail.toml",
                path.display()
            );
        }
    }
    let mut config: Option<MailConfig> = None;
    for path in user {
        let Some(text) = read(path)? else {
            continue;
        };
        owned_by_user(path)?;
        let layer: Layer =
            toml::from_str(&text).with_context(|| format!("mail config {}", path.display()))?;
        let Some(table) = layer.mail else {
            continue;
        };
        check(&table).with_context(|| format!("mail config {}", path.display()))?;
        let merged = config.get_or_insert_with(MailConfig::default);
        if table.notmuch_config.is_some() {
            merged.notmuch_config = table.notmuch_config;
        }
        if table.notmuch.is_some() {
            merged.notmuch = table.notmuch;
        }
        if table.identity.is_some() {
            merged.identity = table.identity;
        }
        if table.deny_file.is_some() {
            merged.deny_file = table.deny_file;
        }
        if let Some(extra) = table.attribution {
            merged.attribution = extra;
        }
    }
    if config
        .as_ref()
        .is_some_and(|c| c.identity.is_some() && c.deny_file.is_none())
    {
        bail!("mail config: identity is set without deny_file, which would deny nothing");
    }
    Ok(config)
}

fn read(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("mail config {}: cannot read", path.display())),
    }
}

fn owned_by_user(path: &Path) -> Result<()> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("mail config {}: cannot stat", path.display()))?;
    if meta.uid() != crate::euid() {
        bail!("mail config {}: not owned by this user", path.display());
    }
    if meta.mode() & 0o022 != 0 {
        bail!("mail config {}: group or world writable", path.display());
    }
    Ok(())
}

fn check(table: &Table) -> Result<()> {
    for (key, path) in [
        ("notmuch_config", &table.notmuch_config),
        ("deny_file", &table.deny_file),
    ] {
        if path.as_deref().is_some_and(|p| !p.is_absolute()) {
            bail!("{key} must be an absolute path");
        }
    }
    if let Some(bin) = &table.notmuch {
        if !bin.is_absolute() && bin.components().count() != 1 {
            bail!("notmuch must be an absolute path or a bare name on PATH");
        }
    }
    if let Some(identity) = &table.identity {
        if !is_bare_address(identity) {
            bail!("identity must be a bare address such as owner@example.test");
        }
    }
    Ok(())
}

/// `local@domain` with no display name, brackets or spaces.
pub(crate) fn is_bare_address(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !text
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>\"(),;:[]\\".contains(c))
        && !domain.contains('@')
}

impl MailConfig {
    /// The notmuch binary to run.
    #[must_use]
    pub fn notmuch_bin(&self) -> &Path {
        self.notmuch.as_deref().unwrap_or(Path::new("notmuch"))
    }
}
