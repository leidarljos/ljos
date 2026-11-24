//! A policy check that used to stop a command.
//!
//! The attempted input is stored. The command prints `label <name>:` on
//! stderr. The effect the check was guarding does not run.
//! A missing file, a failed read, or a failed authentication is not one of
//! these: that stays a non-zero exit, with the same stored label.

use std::fmt;

/// The label a policy check emits.
#[derive(Debug)]
pub struct PolicyLabel {
    pub label: &'static str,
    pub detail: String,
    /// Process status. 0 leaves the note as a note. Any other value is the exit.
    pub status: u8,
}

impl fmt::Display for PolicyLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for PolicyLabel {}

/// Store `detail` under `label` and return it as a policy result.
/// Callers that use `?` stop before the effect. The CLI prints the label.
pub fn labelled(label: &'static str, detail: impl Into<String>) -> anyhow::Error {
    refused(label, detail, 0)
}

/// Store `detail` under `label` and return it with `status`.
/// The CLI prints the label and exits with that status.
pub fn refused(label: &'static str, detail: impl Into<String>, status: u8) -> anyhow::Error {
    let detail = detail.into();
    crate::memory_port::record_label(label, &detail, "labelled");
    anyhow::Error::new(PolicyLabel {
        label,
        detail,
        status,
    })
}

/// Store `label` on a real failure and return `err` unchanged.
/// The process still exits non-zero.
pub fn failed(label: &'static str, err: anyhow::Error) -> anyhow::Error {
    crate::memory_port::record_label(label, &format!("{err:#}"), "failed");
    err
}

/// `Some` when `err` is a policy label.
#[must_use]
pub fn as_policy(err: &anyhow::Error) -> Option<&PolicyLabel> {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<PolicyLabel>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_policy_label_keeps_its_sentence_and_its_name() {
        let err = refused("seat-unresolved", "pass seat, your own bot name", 1);
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "seat-unresolved");
        assert_eq!(label.status, 1);
        assert!(err.to_string().contains("pass seat"), "{err}");
    }

    #[test]
    fn a_failed_read_is_not_a_policy_label() {
        let err = failed(
            "effect-unavailable",
            anyhow::anyhow!("cannot read the file: os error 2"),
        );
        assert!(as_policy(&err).is_none());
        assert!(err.to_string().contains("os error 2"), "{err}");
    }

    #[test]
    fn a_partial_quote_is_approval_unverified() {
        let err = crate::quoted_yes::read(Some("yes keep the parser"), None, None).unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "approval-unverified");
        assert!(err.to_string().contains("Nothing was written"), "{err}");
    }

    #[test]
    fn an_unknown_memory_id_is_a_review_warning() {
        let err = crate::memories::resolve("missing", &[]).unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "review-input-warning");
    }

    #[test]
    fn a_grade_with_no_id_stays_a_failed_read() {
        let err = crate::graded("", true).unwrap_err();
        assert!(as_policy(&err).is_none(), "{err}");
        assert!(err.to_string().contains("atom id is required"), "{err}");
    }

    #[test]
    fn an_empty_message_is_an_input_warning() {
        let err = crate::mail::send(&crate::mail::Send {
            seat: Some("beta"),
            group: None,
            text: "  ",
            interrupt: false,
            issue: None,
            new_seat: false,
        })
        .unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "message-input-warning");
    }

    #[test]
    fn a_self_send_is_a_routing_warning() {
        let err = crate::mail::recipients("alfa", Some("alfa"), None, &[]).unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "routing-warning");
    }

    #[test]
    fn an_empty_quote_on_a_grant_is_unverified() {
        let err = crate::seat_roster::grant(
            "alfa",
            "close=own",
            "  ",
            "2026-01-01T00:00:00Z",
            "beta",
            None,
        )
        .unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "grant-unverified");
        assert!(err.to_string().contains("Nothing was written"), "{err}");
    }

    #[test]
    fn an_empty_lesson_is_an_input_label() {
        let err = crate::packset_write_agent("remember", "  ", None, None).unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "empty-input");
        assert!(
            err.to_string().contains("empty text is not a claim"),
            "{err}"
        );
    }

    #[test]
    fn a_persona_without_a_name_is_an_owner_warning() {
        let err = crate::persona_atom(
            &crate::Persona {
                name: " ".into(),
                anchor: 0.5,
                view: "It reads the work.".into(),
                ..crate::Persona::default()
            },
            "seat",
        )
        .unwrap_err();
        assert_eq!(
            as_policy(&err).expect("label").label,
            "persona-owner-warning"
        );
    }

    #[test]
    fn an_unknown_playbook_name_is_a_warning() {
        let err = crate::parse_playbook_name("not-a-recipe").unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "playbook-warning");
        assert!(err.to_string().contains("unknown name"), "{err}");
    }

    #[test]
    fn an_unseen_recipient_is_labelled() {
        let known = std::collections::BTreeSet::from(["claude-review".to_string()]);
        let err = crate::mail::check_known("claude", &known, false).unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "recipient-unseen");
        assert!(err.to_string().contains("claude-review"), "{err}");
    }

    #[test]
    fn a_rule_without_a_pattern_is_an_input_warning() {
        let err = crate::validate_rule(&crate::Rule {
            pattern: " ".into(),
            verdict: "deny".into(),
            reason: "the command is named".into(),
        })
        .unwrap_err();
        assert_eq!(as_policy(&err).expect("label").label, "rule-input-warning");
    }

    #[test]
    fn a_self_block_is_a_cycle_label() {
        let err = crate::block_edge("probe-aaaa", "probe-aaaa", false).unwrap_err();
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "graph-cycle");
        assert!(err.to_string().contains("cannot block itself"), "{err}");
    }

    /// [`labelled`] stores the attempt under the state directory.
    /// An empty `XDG_STATE_HOME` used to be the relative directory `ljos`,
    /// and a missing home used to be `.local/state/ljos`. `cargo test` runs
    /// in this package, so both were written into the crate.
    #[test]
    fn an_empty_state_home_does_not_write_the_package() {
        let _env = crate::test_env_lock();
        let package = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let previous = std::env::current_dir().expect("working directory");
        struct Back(std::path::PathBuf);
        impl Drop for Back {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _back = Back(previous);
        std::env::set_current_dir(&package).expect("package directory");

        let stray = package.join("ljos");
        let dot = package.join(".local");
        assert!(
            !stray.exists(),
            "the package already holds {}",
            stray.display()
        );
        assert!(!dot.exists(), "the package already holds {}", dot.display());

        let write = |xdg: Option<&std::ffi::OsStr>| {
            match xdg {
                Some(value) => crate::test_env_guards::set_var("XDG_STATE_HOME", value),
                None => crate::test_env_guards::remove_var("XDG_STATE_HOME"),
            }
            crate::test_env_guards::remove_var("HOME");
            crate::test_env_guards::remove_var("VISSUE_ROOT");
            let _ = labelled("input-warning", "state stays out of the package directory");
            crate::memories::bump_recalls(&["state-tree".to_string()]);
            let dir = crate::state_base_dir();
            assert!(dir.is_absolute(), "{}", dir.display());
            assert!(
                !dir.starts_with(&package),
                "state dir {} is inside the package",
                dir.display()
            );
            assert!(!stray.exists(), "wrote {}", stray.display());
            assert!(!dot.exists(), "wrote {}", dot.display());
        };

        write(Some(std::ffi::OsStr::new("")));
        write(None);
        write(Some(package.as_os_str()));
    }
}
