//! `ljos file --parent P` files the child in P's project. A raw
//! `vissue create --parent ljos-gkq2` run from the tracker root filed into
//! Software with a Software- id; a `-p` that names another project than
//! the parent's must not do the same through ljos.

ljos_test_boot::harness!();
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn a_child_is_filed_in_its_parents_project() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("calls.log");
    let stub = dir.path().join("vissue");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\necho \"vissue $*\" >> '{}'\n[ \"$1\" = create ] && echo ljos-new1\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("run")).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let file = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
            .arg("file")
            .args(args)
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join(".state"))
            .env("XDG_RUNTIME_DIR", home.join("run"))
            .env("LJOS_SEAT", "rohit")
            .env("LJOS_TRACKER_GIT", "off")
            .env("PACKSET_URL", "off")
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "ljos file {args:?}: {said}");
        said
    };
    let said = file(&["a child", "--parent", "ljos-gkq2", "-p", "Software"]);
    assert!(said.contains("filed in ljos"), "{said}");
    file(&["another child", "--parent", "ljos-policyd-ab12"]);
    let creates: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("vissue create"))
        .map(str::to_string)
        .collect();
    assert_eq!(creates.len(), 2, "{creates:?}");
    assert!(
        creates[0].starts_with("vissue create -p ljos --parent ljos-gkq2"),
        "{creates:?}"
    );
    assert!(
        creates[1].starts_with("vissue create -p ljos-policyd --parent ljos-policyd-ab12"),
        "{creates:?}"
    );
}
