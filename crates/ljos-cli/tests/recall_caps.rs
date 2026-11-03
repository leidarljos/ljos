//! What `ljos recall` shows for one issue in the cap fixture.
//!
//! The issue cap is 10 because that was the median F1 on the runs in
//! `measurements/recall-caps.txt`. This checks the lines a seat sees for
//! `ufuv-kiln`, including how many own memories are left out. The marks
//! are the ones both recorded passes printed for that issue at this cap.

ljos_test_boot::harness!();

use std::process::Command;

use serde_json::Value;

/// Own marks `ljos recall ufuv-kiln` printed, in order, on both passes in
/// `measurements/recall-caps-stdout.txt` and on a debug build of the same
/// fixture. Ten lines, then the left-out count.
const KILN_OWN: &[&str] = &[
    "m0014", "m0008", "m0011", "m0012", "m0005", "m0001", "m0006", "m0004", "m0002", "m0003",
];

/// Related hits that state the kiln subject. The three that name every
/// title word are `m0047`, `m0048` and `m0049`. The other four name two
/// title words and tie, so which two of them fill the related cap can
/// differ without the cap changing.
const KILN_RELATED: &[&str] = &[
    "m0047", "m0048", "m0049", "m0050", "m0051", "m0052", "m0053",
];

const LEFT_OUT: &str = "(36 more memories on this issue left out)";

#[test]
fn recall_of_ufuv_kiln_lists_ten_own_memories_and_says_how_many_remain() {
    if !ljos_cli::tool_installed("packsetd") {
        panic!("packsetd");
    }
    let daemon = ljos_test_boot::Packsetd::build().spawn();
    let fixture: Value =
        serde_json::from_str(include_str!("../../../scripts/fixtures/recall-caps.json")).unwrap();
    let root = std::env::temp_dir().join(format!("ljos-recall-caps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Software/ufuv")).unwrap();
    std::fs::write(root.join("Software/ufuv/issues.org"), org(&fixture)).unwrap();
    let token = std::fs::read_to_string(daemon.pack_home().join("token"))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_string();
    post_all(daemon.url(), &token, &fixture);
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let stub = bin.join("vissue");
    std::fs::write(
        &stub,
        "#!/bin/sh\nif [ \"$1\" = recall ]; then printf 'tracker row\\n'; fi\nexit 0\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .arg("recall")
        .arg("ufuv-kiln")
        .env_clear()
        .env("HOME", daemon.pack_home())
        .env("PACKSET_HOME", daemon.pack_home())
        .env("PACKSET_URL", daemon.url())
        .env("PACKSET_WORKSPACE", "seat")
        .env("PACKSET_TOKEN_FILE", daemon.pack_home().join("token"))
        .env("VISSUE_ROOT", &root)
        .env("ISSUE_ROOT", &root)
        .env("VISSUE_NO_ROUTE", "1")
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .current_dir(&root)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&root);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "exit {:?}\n{text}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(text.matches(LEFT_OUT).count(), 1, "{text}");
    let left = text.find(LEFT_OUT).unwrap();
    let related = text.find("\n  related:\n").unwrap();
    assert!(left < related, "{text}");
    let own: Vec<&str> = marks_of(&text[..left]);
    let extra: Vec<&str> = marks_of(&text[related..]);
    assert_eq!(own, KILN_OWN, "{text}");
    assert_eq!(extra.len(), 5, "{text}");
    assert!(
        extra.iter().all(|mark| KILN_RELATED.contains(mark)),
        "{extra:?}\n{text}"
    );
    for full in ["m0047", "m0048", "m0049"] {
        assert!(
            extra.contains(&full),
            "{full} missing from {extra:?}\n{text}"
        );
    }
    assert_eq!(text.matches("  - [").count(), 15, "{text}");
}

fn org(fixture: &Value) -> String {
    let mut out = String::from(
        "#+TITLE: ufuv issues\n#+VISSUE: 1\n#+CATEGORY: ufuv\n\
         #+TODO: TODO STARTED BLOCKED | DONE CANCELLED\n\n",
    );
    for issue in fixture["issues"].as_array().unwrap() {
        out.push_str(&format!(
            "* TODO [#B] {}\n:PROPERTIES:\n:ID:         {}\n\
             :CREATED:    [2026-06-01 Mon]\n:END:\n\n",
            issue["title"].as_str().unwrap(),
            issue["id"].as_str().unwrap()
        ));
    }
    out
}

fn post_all(url: &str, token: &str, fixture: &Value) {
    for mem in fixture["memories"].as_array().unwrap() {
        let post = mem["post"].as_u64().unwrap();
        let stamp = format!("2026-06-01T{:02}:{:02}:00.000Z", post / 60, post % 60);
        let mut body = serde_json::json!({
            "workspace": "seat",
            "kind": "lesson",
            "level": "explicit",
            "text": mem["text"],
            "valid_from": stamp,
            "ts": stamp,
        });
        if let Some(issue) = mem["issue"].as_str() {
            body["issue"] = Value::String(issue.to_string());
            body["entities"] = serde_json::json!([format!("issue:{issue}")]);
        }
        let response = ureq::post(&format!("{url}/v1/atoms"))
            .set("Authorization", &format!("Bearer {token}"))
            .send_json(body)
            .unwrap_or_else(|err| panic!("post {}: {err}", mem["id"]));
        assert!(response.status() < 300, "{}", response.status());
    }
}

fn marks_of(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.trim_start().starts_with("- ["))
        .map(|line| {
            line.split("mark ")
                .nth(1)
                .and_then(|rest| rest.split(|c: char| !c.is_ascii_alphanumeric()).next())
                .unwrap_or_else(|| panic!("no mark in {line}"))
        })
        .collect()
}
