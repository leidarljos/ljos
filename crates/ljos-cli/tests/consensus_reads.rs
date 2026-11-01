//! One `ljos consensus` reads each ticket of the family once. The tracker
//! here is a fake that writes down every ask; a reading that asks again
//! for a ticket it already read is the cost that grew with the family.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_consensus_reads_each_ticket_once() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let log = dir.path().join("asks.log");
    let children: Vec<String> = (2..=9)
        .map(|n| format!("{{\"id\":\"demo-{n}\"}}"))
        .collect();
    script(
        dir.path(),
        "vissue",
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$1\" in\n  vote) echo '[{{\"agent\":\"ada\",\"choice\":\"ship\"}},{{\"agent\":\"bob\",\"choice\":\"ship\"}}]' ;;\n  show) case \"$2\" in\n    demo-1) echo '{{\"id\":\"demo-1\",\"title\":\"ship\"}}' ;;\n    *) echo '{{\"id\":\"'\"$2\"'\",\"title\":\"part\",\"parent\":\"demo-1\"}}' ;;\n  esac ;;\n  children) case \"$2\" in\n    demo-1) echo '[{}]' ;;\n    *) echo '[]' ;;\n  esac ;;\n  consensus) echo 'consensus: ship 2 of 2, settled' ;;\n  *) exit 1 ;;\nesac\n",
            log.display(),
            children.join(",")
        ),
    );
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["consensus", "demo-1", "--no-file"])
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".state"))
        .env("XDG_RUNTIME_DIR", &home)
        .env("VISSUE_ROOT", home.join("tracker"))
        .env("PACKSET_URL", "off")
        .env("LJOS_SEAT", "ada")
        .env("LJOS_UNSIGNED_BALLOTS", "allow")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let asks = std::fs::read_to_string(&log).unwrap();
    let mut seen = std::collections::BTreeMap::new();
    for ask in asks.lines() {
        *seen.entry(ask).or_insert(0) += 1;
    }
    let again: Vec<String> = seen
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(ask, n)| format!("{ask} x{n}"))
        .collect();
    assert!(again.is_empty(), "asked again: {again:?}\n{asks}");
    let shows = asks.lines().filter(|a| a.starts_with("show ")).count();
    assert_eq!(shows, 9, "{asks}");
}
