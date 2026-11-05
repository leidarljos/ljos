//! Explicit command modes for the existing strict integration group.
use std::cell::Cell;

thread_local! {
    static MODE: Cell<&'static str> = const { Cell::new("strict") };
}

pub struct Mode {
    previous: &'static str,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Drop for Mode {
    fn drop(&mut self) {
        MODE.with(|slot| slot.set(self.previous));
    }
}

pub fn select(mode: &'static str) -> Mode {
    assert!(matches!(mode, "strict" | "advisory"));
    Mode {
        previous: MODE.with(|slot| slot.replace(mode)),
        _thread: std::marker::PhantomData,
    }
}

pub fn advisory() -> Mode {
    select("advisory")
}

/// Child entries for the calling thread's explicitly selected mode.
pub fn environment() -> [(&'static str, &'static str); 1] {
    MODE.with(|slot| [("LJOS_GUARD_MODE", slot.get())])
}

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    command.envs(environment());
    command
}

#[cfg(test)]
mod tests {
    use super::{advisory, command, environment, select};
    use std::ffi::OsStr;

    fn command_mode() -> String {
        command("fixture-command")
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("LJOS_GUARD_MODE"))
            .and_then(|(_, value)| value)
            .expect("the command carries an explicit mode")
            .to_str()
            .expect("the mode is UTF-8")
            .to_owned()
    }

    #[test]
    fn child_commands_start_in_strict_mode() {
        assert_eq!(command_mode(), "strict");
    }

    #[test]
    fn nested_modes_restore_the_child_command_mode() {
        assert_eq!(command_mode(), "strict");
        {
            let _advisory = advisory();
            assert_eq!(command_mode(), "advisory");
            {
                let _strict = select("strict");
                assert_eq!(command_mode(), "strict");
            }
            assert_eq!(command_mode(), "advisory");
        }
        assert_eq!(command_mode(), "strict");
    }

    #[test]
    fn parallel_threads_keep_independent_command_modes() {
        let _advisory = advisory();
        let other = std::thread::spawn(|| {
            assert_eq!(command_mode(), "strict");
            let _strict = select("strict");
            assert_eq!(command_mode(), "strict");
        });
        other.join().expect("the independent mode assertions hold");
        assert_eq!(command_mode(), "advisory");
    }

    #[test]
    fn cleared_child_environment_retains_explicit_strict_mode() {
        let _strict = select("strict");
        let mut child = command("fixture-command");
        child
            .arg("fixture-argument")
            .env("CLEARED_FIXTURE_ENTRY", "discarded")
            .env_clear()
            .envs(environment());
        assert_eq!(child.get_program(), OsStr::new("fixture-command"));
        assert_eq!(
            child.get_args().collect::<Vec<_>>(),
            [OsStr::new("fixture-argument")]
        );
        assert_eq!(
            child.get_envs().collect::<Vec<_>>(),
            [(OsStr::new("LJOS_GUARD_MODE"), Some(OsStr::new("strict")))]
        );
    }

    #[test]
    fn cleared_child_environment_retains_advisory_and_later_overrides() {
        let _advisory = advisory();
        let mut child = command("fixture-command");
        child
            .arg("fixture-argument")
            .env("CLEARED_FIXTURE_ENTRY", "discarded")
            .env_clear()
            .envs(environment());
        assert_eq!(
            child.get_envs().collect::<Vec<_>>(),
            [(OsStr::new("LJOS_GUARD_MODE"), Some(OsStr::new("advisory")))]
        );
        child.env("LJOS_GUARD_MODE", "strict");
        child.env("LATER_FIXTURE_ENTRY", "retained");
        let entries: Vec<_> = child.get_envs().collect();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&(OsStr::new("LJOS_GUARD_MODE"), Some(OsStr::new("strict")))));
        assert!(entries.contains(&(
            OsStr::new("LATER_FIXTURE_ENTRY"),
            Some(OsStr::new("retained"))
        )));
        assert_eq!(child.get_program(), OsStr::new("fixture-command"));
        assert_eq!(
            child.get_args().collect::<Vec<_>>(),
            [OsStr::new("fixture-argument")]
        );
    }
}
