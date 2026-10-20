//! The roots ljos trusts for https.
//!
//! ureq 2.12.1 depends on webpki-roots 0.26.11, which re-exports
//! TLS_SERVER_ROOTS from webpki-roots 1. cargo deny still reports
//! RUSTSEC-2026-0381 against 0.26.11 by version, and deny.toml ignores it
//! for that reason. This test is what makes the ignore safe: the root set
//! in the build is the one without the emSign root the advisory names.

#[test]
fn the_distrusted_emsign_root_is_not_trusted() {
    let name: &[u8] = b"emSign ECC Root CA - G3";
    let trusted = webpki_roots::TLS_SERVER_ROOTS
        .iter()
        .any(|root| root.subject.as_ref().windows(name.len()).any(|w| w == name));
    assert!(
        !trusted,
        "webpki-roots still ships the emSign ECC Root CA - G3 root"
    );
    // The roots are there at all, so the search above looked at something.
    assert!(webpki_roots::TLS_SERVER_ROOTS.len() > 100);
}
