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

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    MODE.with(|slot| {
        command.env("LJOS_GUARD_MODE", slot.get());
    });
    command
}

#[cfg(test)]
mod tests {
    use super::{advisory, command, select};
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
}
