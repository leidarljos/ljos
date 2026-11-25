//! A `curl` that stands in for crates.io, so the version lookup in doctor
//! and in a sitting gets an answer without the network.
//!
//! A crate is answered with the version the program of that name on the
//! seat's `PATH` prints, which is what an up-to-date seat reads. Any other
//! URL fails the way curl does when the host does not resolve.

use std::path::Path;
use std::time::Duration;

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
case $url in
  https://crates.io/api/v1/crates/*) name=${{url#https://crates.io/api/v1/crates/}} ;;
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
    );
    std::fs::create_dir_all(bin).unwrap_or_else(|err| panic!("create {}: {err}", bin.display()));
    ljos_cli::install_executable(&bin.join("curl"), body)
        .unwrap_or_else(|err| panic!("write the crates.io stand-in: {err}"));
}
