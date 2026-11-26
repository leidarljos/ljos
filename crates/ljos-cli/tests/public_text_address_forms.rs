//! A fixed sample of every non-public address literal, after each kind of
//! separator, with each kind of prefix and suffix and in each letter case,
//! is refused by all three screens. Every Rust path, version and
//! lone `::` the docs keep passes in every wrapping.

use ljos_cli::public_text::{classify, Class, Screen};
use ljos_cli::scrub::{scan_body, scan_public};

const IPV4: &[&str] = &[
    "127.0.0.1",
    "0.0.0.0",
    "10.0.0.1",
    "172.16.0.1",
    "172.31.255.254",
    "192.168.1.1",
    "169.254.1.1",
    "100.64.0.1",
    "192.0.2.1",
    "198.51.100.1",
    "203.0.113.1",
    "198.18.0.1",
    "198.19.1.1",
    "100.127.255.1",
    "224.0.0.0",
    "224.0.0.1",
    "239.255.255.250",
];

const IPV6: &[&str] = &[
    "::1",
    "fe80::1",
    "fe80::abcd",
    "fd00::1",
    "fc00::abcd",
    "fd12:3456:789a::1",
    "fec0::1",
    "ff02::1",
    "2001:db8::1",
    "2001:0db8:0000:0000:0000:0000:0000:0001",
    "2001:2::1",
    "::ffff:10.0.0.1",
    "::ffff:127.0.0.1",
    "::ffff:a00:1",
    "::7f00:1",
    "::a00:1",
    "fead::beef",
    "fead::beef%eth0",
    "fe80::1%eth0",
    "febe::cafe",
    "feab::dead",
    "febf::face",
    "fece::face",
    "feed::beef",
    "ffee::beef",
    "fc00::1",
    "fd00:beef::1",
    "fd00::",
    "2001:db8::dead",
    "2001:2::dead",
];

const SEPARATORS: &[&str] = &[
    "", " ", ".", ":", "::", "=", "(", "[", "`", "/", ",", "'", "\"", "_",
];

/// Letters, digits, hex letters and punctuation.
const PREFIXES: &[&str] = &["xy", "12", "ab", "!"];

const SUFFIXES: &[&str] = &[
    "", "x", "s", "th", "g", "z", "0", "5", "99", "a", "e", "abc", "beef", "f", ".", ",", ":", ")",
    "]", "!", "%eth0", "/24", ":8080", "-",
];

fn title(text: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in text.chars() {
        if c.is_ascii_alphabetic() {
            out.push(if up {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            });
            up = !up;
        } else {
            out.push(c);
        }
    }
    out
}

/// Spellings the cartesian product does not build: a word in front of an
/// address, or a sentence that still contains one.
const EXTRA: &[&str] = &[
    "ssh 0.0.0.0",
    "ssh 224.0.0.0",
    "see db.fe80::1 now",
    "add.fe80::1%eth0",
    "cafe.fd00::1",
    "v10.0.0.1",
    "release v10.0.0.5",
];

fn push_cases(out: &mut Vec<String>, form: &str) {
    out.push(form.to_ascii_uppercase());
    out.push(title(form));
    out.push(form.to_string());
}

/// One form for every literal, prefix, separator and suffix, then a fixed
/// sample of the product. The bare literal is included: every generated
/// form otherwise starts with a prefix.
fn forms() -> Vec<String> {
    let literals: Vec<&str> = IPV4.iter().copied().chain(IPV6.iter().copied()).collect();
    let mut out = Vec::new();
    for (i, literal) in literals.iter().enumerate() {
        push_cases(
            &mut out,
            &format!(
                "{}{}{literal}{}",
                PREFIXES[i % PREFIXES.len()],
                SEPARATORS[i % SEPARATORS.len()],
                SUFFIXES[i % SUFFIXES.len()]
            ),
        );
        push_cases(&mut out, literal);
    }
    for (i, prefix) in PREFIXES.iter().enumerate() {
        let literal = literals[i % literals.len()];
        push_cases(
            &mut out,
            &format!(
                "{prefix}{}{literal}{}",
                SEPARATORS[i % SEPARATORS.len()],
                SUFFIXES[i % SUFFIXES.len()]
            ),
        );
    }
    for (i, separator) in SEPARATORS.iter().enumerate() {
        let literal = literals[i % literals.len()];
        push_cases(
            &mut out,
            &format!(
                "{}{separator}{literal}{}",
                PREFIXES[i % PREFIXES.len()],
                SUFFIXES[i % SUFFIXES.len()]
            ),
        );
    }
    for (i, suffix) in SUFFIXES.iter().enumerate() {
        let literal = literals[i % literals.len()];
        push_cases(
            &mut out,
            &format!(
                "{}{}{literal}{suffix}",
                PREFIXES[i % PREFIXES.len()],
                SEPARATORS[i % SEPARATORS.len()]
            ),
        );
    }
    let mut state: u64 = 0x0a11_5eed;
    for _ in 0..64 {
        state = state.wrapping_mul(0x5851_f42d_4c95_7f2d).wrapping_add(1);
        let literal = literals[(state as usize) % literals.len()];
        let prefix = PREFIXES[((state >> 8) as usize) % PREFIXES.len()];
        let separator = SEPARATORS[((state >> 16) as usize) % SEPARATORS.len()];
        let suffix = SUFFIXES[((state >> 24) as usize) % SUFFIXES.len()];
        push_cases(&mut out, &format!("{prefix}{separator}{literal}{suffix}"));
    }
    for extra in EXTRA {
        push_cases(&mut out, extra);
    }
    out.sort();
    out.dedup();
    out
}

fn verdicts(text: &str) -> [bool; 3] {
    [
        classify(text, &Screen::default()) == Some(Class::Address),
        !scan_body(text, 1).is_empty(),
        !scan_public(text).is_empty(),
    ]
}

#[test]
fn every_non_public_literal_is_refused_whatever_surrounds_it() {
    let forms = forms();
    let missed: Vec<String> = forms
        .iter()
        .filter_map(|form| {
            let verdict = verdicts(form);
            (verdict != [true; 3]).then(|| format!("{form:?} {verdict:?}"))
        })
        .collect();
    assert!(
        missed.is_empty(),
        "{} of {} forms not refused by every screen, such as:\n{}",
        missed.len(),
        forms.len(),
        missed[..missed.len().min(40)].join("\n")
    );
}

/// User time of this process. A loaded core changes wall time and leaves
/// this alone, so a linear scan is compared here rather than to a wall cap.
fn user_cpu() -> std::time::Duration {
    let mut usage = unsafe { std::mem::zeroed::<libc::rusage>() };
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    assert_eq!(rc, 0, "getrusage");
    let secs = u64::try_from(usage.ru_utime.tv_sec).unwrap_or(0);
    let micros = u64::try_from(usage.ru_utime.tv_usec).unwrap_or(0);
    std::time::Duration::from_secs(secs) + std::time::Duration::from_micros(micros)
}

/// `long` is the same shape as `short` and about `long.len() / short.len()`
/// times as long. Both must classify as `None`. User time may grow with
/// that ratio, with slack, and a many-second wall cap only catches a hang.
fn assert_linear_scan(short: &str, long: &str) {
    let scale = long.len() as f64 / short.len() as f64;
    let wall = std::time::Instant::now();
    let before = user_cpu();
    assert_eq!(classify(short, &Screen::default()), None);
    let short_cpu = user_cpu().saturating_sub(before);
    let before = user_cpu();
    assert_eq!(classify(long, &Screen::default()), None);
    let long_cpu = user_cpu().saturating_sub(before);
    assert!(
        wall.elapsed() < std::time::Duration::from_secs(30),
        "the scan hung after {:?}",
        wall.elapsed()
    );
    if long_cpu <= std::time::Duration::from_millis(20) {
        return;
    }
    let base = short_cpu.max(std::time::Duration::from_millis(5));
    let limit = base.mul_f64(scale * 1.75);
    assert!(
        long_cpu <= limit,
        "user cpu {long_cpu:?} on {} bytes is past {limit:?}, scaled from {short_cpu:?} on {} bytes",
        long.len(),
        short.len()
    );
}

#[test]
fn the_ipv4_scan_is_linear_on_alphanumeric_runs() {
    assert_linear_scan(&"a1".repeat(1_250), &"a1".repeat(5_000));
}

const KEPT: &[&str] = &[
    "bb8::",
    "bb8::{Pool}",
    "bb8::Pool",
    "f64::E",
    "f64::EPSILON",
    "std::fmt",
    "std::fs::read",
    "d3::<T>",
    "dead::Beef",
    "Dead::Beef",
    "Face::Fade",
    "Face::<T>::new()",
    "Vec::<u8>::new()",
    "Self::new()",
    "<T>::bad()",
    "u8::MAX",
    "i32::from(x)",
    "x86_64::foo",
    "crate::de::ad1",
    "Foo::{a}",
    "Option::None",
    "Token::Eof",
    "v1.2.3.4",
    "v1.10.0.0",
    "rev1.2.3.4",
    "version 3.2.1.4",
    "release v1.10.0.0",
    "1.2.3.4 is the version",
    "8.8.8.8",
    "255.255.255.0",
    "term :: description",
    "term:: description",
    "Notes ::",
    "x/::",
    "ip ::",
    "(::)",
    "`::`",
    "'::'",
    "a, ::",
    "[::]",
    "00:1a:2b:3c:4d:5e",
    "00:1a:2b:ff:fe:3c:4d:5e",
    "MD5:16:27:ac:7f:83:48",
    "ssh fingerprint 16:27:ac:7f:83:48",
    "SHA1 Fingerprint=AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:01",
    "SHA-256 Fingerprint=AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89",
];

const WRAPPINGS: &[(&str, &str)] = &[
    ("", ""),
    ("`", "`"),
    ("use ", ";"),
    ("(", ")"),
    ("see ", " here"),
];

#[test]
fn every_kept_path_version_and_lone_double_colon_passes_in_every_wrapping() {
    let mut refused = Vec::new();
    for kept in KEPT {
        for (open, close) in WRAPPINGS {
            let form = format!("{open}{kept}{close}");
            let verdict = verdicts(&form);
            if verdict != [false; 3] {
                refused.push(format!("{form:?} {verdict:?}"));
            }
        }
    }
    assert!(refused.is_empty(), "refused:\n{}", refused.join("\n"));
}

#[test]
fn colon_hex_mac_addresses_and_fingerprints_pass() {
    for form in [
        "00:1a:2b:3c:4d:5e",
        "00:1a:2b:ff:fe:3c:4d:5e",
        "MD5:16:27:ac:7f:83:48",
        "ssh fingerprint 16:27:ac:7f:83:48",
        "SHA1 Fingerprint=AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:01",
        "SHA-256 Fingerprint=AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89",
    ] {
        assert_eq!(verdicts(form), [false; 3], "{form:?}");
    }
}

#[test]
fn a_bare_identifier_double_colon_passes() {
    for form in ["bb8::", "`bb8::`", "see bb8:: here", "(bb8::)"] {
        assert_eq!(verdicts(form), [false; 3], "{form:?}");
    }
    assert_eq!(verdicts("fe80::"), [true; 3]);
}

#[test]
fn a_lone_double_colon_after_an_equals_sign_or_a_dot_is_refused() {
    for form in ["addr=::", "ip=::", "x.::", "a;::", "::bad", "`::a`"] {
        assert_eq!(verdicts(form), [true; 3], "{form:?}");
    }
}

#[test]
fn a_long_run_is_refused_only_when_a_piece_of_it_is_an_address() {
    let digest = "ab".repeat(64);
    let fingerprint = (0..32)
        .map(|i| format!("{:02X}", i * 7 % 256))
        .collect::<Vec<_>>()
        .join(":");
    let dump = (0..40)
        .map(|i| format!("{:02x}", (i * 37 + 160) % 256))
        .collect::<Vec<_>>()
        .join(".");
    for form in [
        digest.clone(),
        format!("{digest}."),
        format!("sha256:{}", "c".repeat(64)),
        ".".repeat(80),
        format!("1.2.3.{}", "4".repeat(70)),
        "1.3.6.1.4.1.311.21.8.1234567.1234567.1234567.1234567.1234567.1234567.1234".to_string(),
        format!("Introduction{}12", ".".repeat(70)),
        dump,
        format!("SHA-256 Fingerprint={fingerprint}."),
        format!("The key is {fingerprint}."),
    ] {
        assert_eq!(verdicts(&form), [false; 3], "{form:?}");
    }
    for form in [
        format!("{}::1", "a".repeat(70)),
        format!("{}10.0.0.1", "1.".repeat(40)),
        format!("{}:fe80::1", "ab:".repeat(30)),
        format!("{}:10.0.0.1", "ab:".repeat(30)),
        format!("{fingerprint}:fe80::1."),
    ] {
        assert_eq!(verdicts(&form), [true; 3], "{form:?}");
    }
}

#[test]
fn every_four_parts_of_a_dotted_run_are_read_as_inet_aton_reads_them() {
    let padded = format!("{}12.0.0.1", "0".repeat(57));
    for form in [
        "1.012.0.0.1",
        "1.0177.0.0.1",
        "9.0x0a.0.0.1",
        "1.0300.0250.0.1",
        padded.as_str(),
        "1.10.0.0.5",
        "v1.10.0.0.5",
        "1.2.3.4.5.6.0xc0.0250.1.1",
    ] {
        assert_eq!(verdicts(form), [true; 3], "{form:?}");
    }
    for form in [
        "1.3.6.1.4.1.311.21.8",
        "OID 1.3.6.1.4.1.311.21.8.1234567.1",
        "attribute 2.5.4.3.1",
        "v1.10.0.0",
        "release v1.10.0.0",
        "version 1.2.3.4.5",
        "1.0.1.0.1.0.1",
        "0x10000000a.0.0.1",
    ] {
        assert_eq!(verdicts(form), [false; 3], "{form:?}");
    }
    let long = format!("{}12.0.0.1", "0".repeat(20_000));
    let started = std::time::Instant::now();
    assert_eq!(classify(&long, &Screen::default()), Some(Class::Address));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "a {}-character part took {:?}",
        long.len(),
        started.elapsed()
    );
}

#[test]
fn a_long_colon_hex_run_is_read_in_linear_time() {
    let colon = |groups: usize| {
        (0..groups)
            .map(|i| format!("{:02x}", i % 256))
            .collect::<Vec<_>>()
            .join(":")
    };
    assert_linear_scan(&colon(1_700), &colon(6_800));
    assert_linear_scan(&"ab:".repeat(1_700), &"ab:".repeat(6_800));
    assert_linear_scan(&"1.".repeat(2_500), &"1.".repeat(10_000));
}
