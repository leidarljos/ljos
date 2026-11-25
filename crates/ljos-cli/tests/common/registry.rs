//! A `curl` that stands in for crates.io, so the version lookup in doctor
//! and in a sitting gets an answer without the network.
//!
//! A crate is answered with the version the program of that name on the
//! seat's `PATH` prints, which is what an up-to-date seat reads. Any other
//! URL fails the way curl does when the host does not resolve. Every URL
//! asked is written to `bin/.curl-asked`, one a line.

use std::path::{Path, PathBuf};
use std::time::Duration;

const CRATES: &str = "https://crates.io/api/v1/crates/";

fn log_of(bin: &Path) -> PathBuf {
    bin.join(".curl-asked")
}

/// The URLs the seat asked the stand-in for, in order.
#[allow(dead_code)]
pub fn asked(bin: &Path) -> Vec<String> {
    match std::fs::read_to_string(log_of(bin)) {
        Ok(text) => text.lines().map(str::to_owned).collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!("read {}: {err}", log_of(bin).display()),
    }
}

/// Fails when the seat asked curl for anything but a crates.io lookup, or
/// asked for none: either way the seat read the network or skipped the
/// lookup this stand-in answers.
#[allow(dead_code)]
pub fn assert_only_registry(bin: &Path, place: &str) {
    let asked = asked(bin);
    let off: Vec<&String> = asked
        .iter()
        .filter(|url| !url.starts_with(CRATES))
        .collect();
    assert!(
        off.is_empty(),
        "{place}: the seat asked curl for a URL this test answers from no stand-in: {off:?}"
    );
    assert!(
        !asked.is_empty(),
        "{place}: the seat asked crates.io for no version, so doctor's lookup did not run"
    );
}

/// Write the stand-in as `bin/curl`. Each answer waits `delay` first.
pub fn stand_in(bin: &Path, delay: Duration) {
    let wait = if delay.is_zero() {
        String::new()
    } else {
        format!("sleep {}.{:03}\n", delay.as_secs(), delay.subsec_millis())
    };
    let body = format!(
        r#"#!/bin/sh
for arg; do url=$arg; done
printf '%s\n' "$url" >> '{log}'
case $url in
  {crates}*) name=${{url#{crates}}} ;;
  *) echo "curl: (6) Could not resolve host: this test seat has no network" >&2; exit 6 ;;
esac
case $name in
  *-cli) program=${{name%-cli}} ;;
  *) program=$name ;;
esac
{wait}version={fallback}
line=$("$program" --version 2>/dev/null </dev/null) || line=
for word in $line; do
  case $word in
    [0-9]*.[0-9]*) version=$word; break ;;
  esac
done
printf '{{"crate":{{"name":"%s","max_version":"%s"}}}}\n' "$name" "$version"
"#,
        fallback = env!("CARGO_PKG_VERSION"),
        log = log_of(bin).display(),
        crates = CRATES,
    );
    std::fs::create_dir_all(bin).unwrap_or_else(|err| panic!("create {}: {err}", bin.display()));
    ljos_cli::install_executable(&bin.join("curl"), body)
        .unwrap_or_else(|err| panic!("write the crates.io stand-in: {err}"));
}
