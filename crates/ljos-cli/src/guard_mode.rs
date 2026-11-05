//! Command decisions are advisory unless strict consent is selected.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardMode {
    Advisory,
    Strict,
}

impl GuardMode {
    fn from_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("strict") => Self::Strict,
            _ => Self::Advisory,
        }
    }
}

#[must_use]
pub fn current() -> GuardMode {
    #[cfg(test)]
    if let Some(mode) = TEST_MODE.with(std::cell::Cell::get) {
        return mode;
    }
    GuardMode::from_value(crate::named_env("LJOS_GUARD_MODE").as_deref())
}

#[must_use]
pub fn is_strict() -> bool {
    current() == GuardMode::Strict
}

pub(crate) fn configure_child(command: &mut std::process::Command) {
    command.env(
        "LJOS_GUARD_MODE",
        match current() {
            GuardMode::Advisory => "advisory",
            GuardMode::Strict => "strict",
        },
    );
}

#[cfg(test)]
thread_local! {
    static TEST_MODE: std::cell::Cell<Option<GuardMode>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestMode {
    previous: Option<GuardMode>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(test)]
impl Drop for TestMode {
    fn drop(&mut self) {
        TEST_MODE.with(|mode| mode.set(self.previous));
    }
}

#[cfg(test)]
pub(crate) fn for_test(mode: GuardMode) -> TestMode {
    TestMode {
        previous: TEST_MODE.with(|slot| slot.replace(Some(mode))),
        _thread: std::marker::PhantomData,
    }
}

#[cfg(test)]
pub(crate) fn fixture_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    configure_child(&mut command);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_advisory_and_strict_is_explicit() {
        assert_eq!(GuardMode::from_value(None), GuardMode::Advisory);
        assert_eq!(GuardMode::from_value(Some("")), GuardMode::Advisory);
        assert_eq!(GuardMode::from_value(Some("advisory")), GuardMode::Advisory);
        assert_eq!(GuardMode::from_value(Some("strict")), GuardMode::Strict);
    }

    #[test]
    fn nested_fixture_modes_restore_without_process_environment_changes() {
        let _advisory = for_test(GuardMode::Advisory);
        assert!(!is_strict());
        {
            let _strict = for_test(GuardMode::Strict);
            assert!(is_strict());
        }
        assert!(!is_strict());
    }

    #[test]
    fn parallel_fixture_modes_belong_to_their_own_threads() {
        let _strict = for_test(GuardMode::Strict);
        let child = std::thread::spawn(|| {
            let _advisory = for_test(GuardMode::Advisory);
            assert!(!is_strict());
        });
        child.join().unwrap();
        assert!(is_strict());
    }

    #[test]
    fn relay_children_receive_the_selected_fixture_mode_explicitly() {
        for (mode, expected) in [
            (GuardMode::Advisory, "advisory"),
            (GuardMode::Strict, "strict"),
        ] {
            let _mode = for_test(mode);
            let mut command = std::process::Command::new("unused-fixture-command");
            configure_child(&mut command);
            let value = command
                .get_envs()
                .find(|(key, _)| *key == "LJOS_GUARD_MODE")
                .and_then(|(_, value)| value)
                .unwrap();
            assert_eq!(value, expected);
        }
    }

    #[test]
    fn advisory_rules_warn_without_requesting_command_consent() {
        let _mode = for_test(GuardMode::Advisory);
        let call = crate::hook_call(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"example command"}}"#,
        );
        for verdict in ["deny", "ask"] {
            let rule = crate::Rule {
                pattern: "fixture-rule".into(),
                verdict: verdict.into(),
                reason: "fixture reason".into(),
            };
            assert!(!rule.blocks());
            let output = crate::hook_output_ruled(&call, "fixture context", Some(&rule));
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert!(value
                .pointer("/hookSpecificOutput/permissionDecision")
                .is_none());
            let context = value
                .pointer("/hookSpecificOutput/additionalContext")
                .and_then(serde_json::Value::as_str)
                .unwrap();
            assert!(context.contains("fixture reason"));
            assert!(context.contains("fixture context"));
            assert!(!output.contains("approval request"));
        }
    }

    #[test]
    fn strict_rules_retain_their_deny_and_native_ask_decisions() {
        let _mode = for_test(GuardMode::Strict);
        let call = crate::hook_call(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"example command"}}"#,
        );
        for verdict in ["deny", "ask"] {
            let rule = crate::Rule {
                pattern: "fixture-rule".into(),
                verdict: verdict.into(),
                reason: "fixture reason".into(),
            };
            assert!(rule.blocks());
            let output = crate::hook_output_ruled(&call, "fixture context", Some(&rule));
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(
                value
                    .pointer("/hookSpecificOutput/permissionDecision")
                    .and_then(serde_json::Value::as_str),
                Some(verdict)
            );
        }
    }

    #[test]
    fn cloud_publication_refusals_apply_in_both_modes() {
        let call = crate::hook_call(
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"git push --force origin main"}}"#,
        );
        let rule = crate::Rule {
            pattern: "cloud-cursor-push".into(),
            verdict: "deny".into(),
            reason: "fixture cloud restriction".into(),
        };
        for mode in [GuardMode::Advisory, GuardMode::Strict] {
            let _mode = for_test(mode);
            assert!(rule.blocks());
            let output = crate::hook_output_ruled(&call, "", Some(&rule));
            assert!(crate::answer_denies(&output));
        }
    }
}
