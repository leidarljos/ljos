//! The optional `[mail]` table.
//!
//! It is read from the same layers as the panel budget, plus
//! `mail.toml` in the ljos config dir. A later layer overrides the keys it
//! sets. Unlike the budget, a layer that does not parse is an error: a
//! typo in `deny_file` must not quietly drop the deny list.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use serde::Deserialize;

/// What the `[mail]` layers say. Every key is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailConfig {
    /// `NOTMUCH_CONFIG` for every read.
    pub notmuch_config: Option<PathBuf>,
    /// The notmuch binary: an absolute path, or a bare name found on PATH.
    pub notmuch: Option<PathBuf>,
    /// The one address a draft's From may carry.
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

/// The `[mail]` config from the usual layers. `None` when no layer has
/// the table.
pub fn load() -> Result<Option<MailConfig>> {
    load_from(&crate::converge::config_paths(Some("mail.toml")))
}

/// The `[mail]` config from `paths`, in order. A missing file is skipped.
pub fn load_from(paths: &[PathBuf]) -> Result<Option<MailConfig>> {
    let mut config: Option<MailConfig> = None;
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
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
    Ok(config)
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
