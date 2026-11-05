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
