//! The mail design doc is public, so it names no real mailbox.
//!
//! Every address and domain in it is under `example.test`, it names no
//! home directory, and it carries the approval file layout the exporter
//! writes.

use std::path::Path;

fn doc() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join("docs/design/mail.md")).expect("docs/design/mail.md")
}

#[test]
fn the_mail_doc_names_the_tools_the_gate_and_the_approval_file() {
    let doc = doc();
    for want in [
        "ljos_mail_search",
        "ljos_mail_show",
        "LJOS_MCP_TOOLS",
        "mailgate",
        "SO_PEERCRED",
        "[mail]",
        "ljos-outbox v1",
        "reachbook-hash",
        "expires",
        "nonce",
        "sha256",
        "display:none",
        "NFKC",
        "sendmail -i -t",
        "hidden_content",
        "not in ljos yet",
    ] {
        assert!(doc.contains(want), "the mail doc does not name {want}");
    }
}

#[test]
fn the_mail_doc_uses_only_example_test_addresses_and_no_home_paths() {
    let doc = doc();
    for word in doc.split(|c: char| c.is_whitespace() || "<>()`\"',;".contains(c)) {
        let word = word.trim_end_matches(['.', ':']);
        if let Some((local, domain)) = word.split_once('@') {
            if local.is_empty() || !domain.contains('.') {
                continue;
            }
            assert!(
                domain == "example.test" || domain.ends_with(".example.test"),
                "the mail doc names an address outside example.test: {word}"
            );
        }
    }
    for banned in ["/home/", "/Users/", "socks5://"] {
        assert!(!doc.contains(banned), "the mail doc names {banned}");
    }
}
