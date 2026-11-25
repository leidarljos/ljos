//! The writers behind `say!`, `tell!` and `emit!`, the panic hook, and the
//! terminal probes for stdout and stderr.
//!
//! Wrapping is decided by the kind of output. Machine output is never
//! wrapped: text written through `emit!`, and everything once
//! [`set_output_kind`] selects machine output, which `ljos` does for `--json`,
//! `--porcelain`, `--out -` and hooks, and `ljos-mcp` does at start. In
//! that mode `Width::stdout` and `Width::stderr` report no terminal, so
//! pages are built at the fixed layout too. Prose written through `say!`
//! or `tell!` wraps in [`shape`] when its stream is a terminal.
//!
//! The workspace `clippy.toml` refuses `print!`, `println!`, `eprint!`,
//! `eprintln!`, `dbg!`, `std::io::stdout` and `std::io::stderr`. The items
//! here are allowed them, each by name, and so are the byte writers that
//! answer a hook or write an export body.

use std::borrow::Cow;
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::output_wrap::Width;
use crate::scan;

/// What a piece of text is, which decides whether it may wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A sentence or page a person reads.
    Prose,
    /// JSON, ids, rows, hook answers and anything else a program reads.
    Machine,
}

static MACHINE_OUTPUT: AtomicBool = AtomicBool::new(false);

/// Set the kind of everything this process writes, on stdout and stderr.
///
/// Each binary calls this once before it can write. `ljos` selects the kind
/// from its arguments, while `ljos-mcp` is always machine output.
pub fn set_output_kind(kind: Kind) {
    MACHINE_OUTPUT.store(kind == Kind::Machine, Ordering::Relaxed);
}

/// Whether [`set_output_kind`] selected machine output.
#[must_use]
pub fn machine_output() -> bool {
    MACHINE_OUTPUT.load(Ordering::Relaxed)
}

/// `text` as it is written to a stream of `width`. Machine text is
/// unchanged. Prose wraps when the stream is a terminal. A prose text that
/// is one JSON value stays whole, and so does each line that is a JSON
/// object or array or a tab-separated record; a doctor row wraps under
/// its name.
#[must_use]
pub fn shape(text: &str, width: Width, kind: Kind) -> Cow<'_, str> {
    if kind == Kind::Machine
        || width.terminal().is_none()
        || serde_json::from_str::<Value>(text.trim()).is_ok()
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.split_inclusive('\n')
            .map(|line| {
                let body = line.trim();
                let json = (body.starts_with('{') || body.starts_with('['))
                    && serde_json::from_str::<Value>(body).is_ok();
                let record = line.contains('\t')
                    && !line.split('\t').next().is_some_and(crate::is_doctor_word);
                if json || record {
                    Cow::Borrowed(line)
                } else {
                    Cow::Owned(scan::wrap_on_terminal(line, width))
                }
            })
            .collect::<String>(),
    )
}

/// Writes prose on stderr through [`shape`]. A line the crate shows a
/// person on stderr goes through here.
#[allow(
    clippy::disallowed_macros,
    reason = "the stderr writer behind say!; shape decides the wrap"
)]
pub fn say_text(text: &str) {
    eprint!("{}", shape(text, Width::stderr(), Kind::Prose));
}

/// [`say_text`] for a writer that must not panic: a panic hook, or the
/// last line before an exit. A failed write is dropped.
#[allow(
    clippy::disallowed_methods,
    reason = "the stderr writer of the panic hook; shape decides the wrap"
)]
fn say_quiet(text: &str) {
    let shown = shape(text, Width::stderr(), Kind::Prose);
    let _ = std::io::stderr().write_all(shown.as_bytes());
}

/// Writes prose on stdout through [`shape`].
pub fn tell_text(text: &str) {
    emit_text(&shape(text, Width::stdout(), Kind::Prose));
}

/// Writes machine text on stdout as it is. Text the crate prints on
/// stdout goes through here or through [`tell_text`].
#[allow(
    clippy::disallowed_macros,
    reason = "the stdout writer behind emit! and tell!"
)]
pub fn emit_text(text: &str) {
    print!("{text}");
}

/// Flushes stdout, for a prompt that waits on stdin or a frame drawn in
/// place.
///
/// # Errors
///
/// The write to stdout failing.
#[allow(
    clippy::disallowed_methods,
    reason = "flushes what emit_text wrote; writes no text of its own"
)]
pub fn flush_stdout() -> std::io::Result<()> {
    std::io::stdout().flush()
}

/// Whether stdout is a terminal.
#[must_use]
#[allow(clippy::disallowed_methods, reason = "asks isatty and writes nothing")]
pub fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Whether stderr is a terminal.
#[must_use]
#[allow(clippy::disallowed_methods, reason = "asks isatty and writes nothing")]
pub fn stderr_is_terminal() -> bool {
    std::io::stderr().is_terminal()
}

/// The text the panic hook writes: the thread, the place and the message,
/// then the backtrace when `RUST_BACKTRACE` asks for one, else the note
/// that names it.
#[must_use]
pub fn panic_text(thread: &str, place: &str, message: &str, backtrace: Option<&str>) -> String {
    let mut text = format!("\nthread '{thread}' panicked at {place}:\n{message}\n");
    match backtrace {
        Some(trace) => {
            text.push_str("stack backtrace:\n");
            text.push_str(trace);
            if !trace.ends_with('\n') {
                text.push('\n');
            }
        }
        None => text.push_str(
            "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n",
        ),
    }
    text
}

/// Sets a panic hook that writes the panic through [`shape`] in place of
/// the standard hook. A hook set later that chains the one it takes, as
/// `arm_gate` does, still writes through this one.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "Box<dyn Any>".to_string()
        };
        let place = info
            .location()
            .map_or_else(String::new, ToString::to_string);
        let thread = std::thread::current();
        let backtrace = match std::env::var("RUST_BACKTRACE").as_deref() {
            Ok("0") | Err(_) => None,
            Ok(_) => Some(std::backtrace::Backtrace::force_capture().to_string()),
        };
        say_quiet(&panic_text(
            thread.name().unwrap_or("<unnamed>"),
            &place,
            &message,
            backtrace.as_deref(),
        ));
    }));
}

/// Writes a top-level error the way a `main` that returns `Result` does,
/// `Error: ` and the error's debug form, through [`shape`].
pub fn say_error(err: &anyhow::Error) {
    say_quiet(&format!("Error: {err:?}\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_text_names_the_thread_place_and_message() {
        let text = panic_text("main", "src/a.rs:3:5", "boom", None);
        assert_eq!(
            text,
            "\nthread 'main' panicked at src/a.rs:3:5:\nboom\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n"
        );
        let traced = panic_text("main", "src/a.rs:3:5", "boom", Some("   0: f"));
        assert!(traced.ends_with("stack backtrace:\n   0: f\n"), "{traced}");
    }

    #[test]
    fn prose_wraps_on_a_terminal_and_machine_text_does_not() {
        let text = "one two three four five six seven eight nine ten eleven\n";
        let narrow = Width::columns(20);
        let wrapped = shape(text, narrow, Kind::Prose);
        assert!(wrapped.lines().count() > 1, "{wrapped}");
        assert!(wrapped.lines().all(|line| line.chars().count() <= 20));
        assert_eq!(shape(text, narrow, Kind::Machine), text);
        assert_eq!(shape(text, Width::FIXED, Kind::Prose), text);
    }

    #[test]
    fn pretty_json_prose_stays_whole_on_either_stream() {
        let text =
            "{\n  \"said\": \"exported forty memories to standard output for the seat\"\n}\n";
        assert_eq!(shape(text, Width::columns(20), Kind::Prose), text);
    }
}
