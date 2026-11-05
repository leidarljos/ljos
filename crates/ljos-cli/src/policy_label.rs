//! A policy check that used to stop a command.
//!
//! The attempted input is stored. The command prints `label <name>:` and
//! exits 0. The effect the check was guarding does not run.
//! A missing file, a failed read, or a failed authentication is not one of
//! these: that stays a non-zero exit, with the same stored label.

use std::fmt;

/// The label a policy check emits.
#[derive(Debug)]
pub struct PolicyLabel {
    pub label: &'static str,
    pub detail: String,
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
    let detail = detail.into();
    crate::memory_port::record_label(label, &detail, "labelled");
    anyhow::Error::new(PolicyLabel { label, detail })
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
        let err = labelled("seat-unresolved", "pass seat, your own bot name");
        let label = as_policy(&err).expect("label");
        assert_eq!(label.label, "seat-unresolved");
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
}
