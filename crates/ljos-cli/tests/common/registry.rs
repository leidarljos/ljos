//! A `curl` that stands in for crates.io, so the version lookup in doctor
//! and in a sitting gets an answer without the network.
//!
//! A crate is answered with the version the program of that name on the
//! seat's `PATH` prints, which is what an up-to-date seat reads. A call
//! that names anything but one crate's version fails the way curl does
//! when the host does not resolve. The stand-in reads curl's arguments:
//! every URL a call names, positional or after `--url`, is written to
//! `bin/.curl-asked`, one line a call with its URLs in order.
//!
//! A config file can name URLs the arguments do not. curl reads one
//! unless `-q` or `--disable` is the first argument, and the file it
//! reads is not a fixed list. The stand-in refuses a call that does not
//! start that way. It also refuses `-K` and `--config` in any form and
//! any option it does not know. What it refused is written on the call's
//! line after `refused:`.

use std::path::{Path, PathBuf};
use std::time::Duration;

const CRATES: &str = "https://crates.io/api/v1/crates/";

/// Short flags of curl the stand-in lets through. None of them changes
/// which URLs a call reads.
const SHORT_FLAGS: &str = "fsS";

/// Short options of curl the stand-in lets through that take a value,
/// either attached or as the next argument.
const SHORT_VALUED: &str = "Am";

/// Long flags of curl the stand-in lets through.
const LONG_FLAGS: &[&str] = &["--fail", "--silent", "--show-error"];

/// Long options of curl the stand-in lets through that take the next
/// argument as their value, other than `--url`.
const LONG_VALUED: &[&str] = &["--connect-timeout", "--max-time", "--user-agent"];

fn log_of(bin: &Path) -> PathBuf {
    bin.join(".curl-asked")
}

/// The URLs each call of the stand-in named, and what it refused, one
/// entry a call, in order.
#[allow(dead_code)]
pub fn asked(bin: &Path) -> Vec<Vec<String>> {
    match std::fs::read_to_string(log_of(bin)) {
        Ok(text) => text
            .lines()
            .map(|call| call.split_whitespace().map(str::to_owned).collect())
            .collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => panic!("read {}: {err}", log_of(bin).display()),
    }
}

/// Whether `url` is crates.io's endpoint for one crate's versions, and no
/// other path under it.
#[allow(dead_code)]
fn version_endpoint(url: &str) -> bool {
    url.strip_prefix(CRATES).is_some_and(|name| {
        name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

/// Fails when a call to curl named anything but one crate's version
/// endpoint, or when the seat made no call: either way the seat read the
/// network or skipped the lookup this stand-in answers.
#[allow(dead_code)]
pub fn assert_only_registry(bin: &Path, place: &str) {
    let calls = asked(bin);
    let off: Vec<&Vec<String>> = calls
        .iter()
        .filter(|urls| !matches!(urls.as_slice(), [url] if version_endpoint(url)))
        .collect();
    assert!(
        off.is_empty(),
        "{place}: the seat asked curl for something other than one crates.io version: {off:?}"
    );
    assert!(
        !calls.is_empty(),
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
set -f
urls=
refused=
next=
ended=
first=1
quiet=
for arg; do
  if [ -n "$next" ]; then
    [ "$next" = url ] && urls="$urls $arg"
    next=
    continue
  fi
  if [ -n "$ended" ]; then
    urls="$urls $arg"
    continue
  fi
  case $arg in
    -q|--disable)
      if [ -n "$first" ]; then quiet=1; else refused="$refused refused:$arg"; fi ;;
    --) ended=1 ;;
    --url) next=url ;;
    {long_valued}) next=value ;;
    {long_flags}) ;;
    -[!-]*)
      rest=${{arg#-}}
      while [ -n "$rest" ]; do
        flag=${{rest%"${{rest#?}}"}}
        rest=${{rest#?}}
        case $flag in
          [{short_flags}]) ;;
          [{short_valued}]) [ -z "$rest" ] && next=value; break ;;
          *) refused="$refused refused:$arg"; break ;;
        esac
      done ;;
    ''|-*) refused="$refused refused:$arg" ;;
    *) urls="$urls $arg" ;;
  esac
  first=
done
[ -z "$next" ] || refused="$refused refused:no-value"
# curl reads a config file unless -q or --disable is the first argument.
# Which file that is depends on the curl version, so this does not keep a list.
if [ -z "$quiet" ]; then
  refused="$refused refused:config"
fi
line="$urls$refused"
printf '%s\n' "${{line# }}" >> '{log}'
if [ -n "$refused" ]; then
  echo "curl: the crates.io stand-in refuses${{refused}}" >&2
  exit 2
fi
set -- $urls
url=$1
[ $# -eq 1 ] || url=
case $url in
  {crates}*) name=${{url#{crates}}} ;;
  *) name= ;;
esac
case $name in
  ''|[!A-Za-z]*|*[!A-Za-z0-9_-]*)
    echo "curl: (6) Could not resolve host: this test seat has no network" >&2; exit 6 ;;
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
        long_valued = LONG_VALUED.join("|"),
        long_flags = LONG_FLAGS.join("|"),
        short_flags = SHORT_FLAGS,
        short_valued = SHORT_VALUED,
    );
    std::fs::create_dir_all(bin).unwrap_or_else(|err| panic!("create {}: {err}", bin.display()));
    ljos_cli::install_executable(&bin.join("curl"), body)
        .unwrap_or_else(|err| panic!("write the crates.io stand-in: {err}"));
}
