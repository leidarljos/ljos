//! Check that test sources do not hardcode machine home or runtime paths and CLI help probes leave a sentinel HOME untouched.

ljos_test_boot::harness!();

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn collect_entries(root: &Path, cur: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(cur) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        out.push(rel);
        if path.is_dir() {
            collect_entries(root, &path, out);
        }
    }
}

#[test]
fn test_sources_do_not_hardcode_machine_home_or_runtime_dirs() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs(&root.join("crates"), &mut files);
    files.sort();

    let mut hits = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if rel == "crates/ljos-cli/tests/home_hermetic.rs"
            || rel == "crates/ljos-cli/tests/docs_truth.rs"
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let is_integration = rel.contains("/tests/");
        for (idx, line) in text.lines().enumerate() {
            if line.contains("/home/ubuntu") {
                hits.push(format!("{rel}:{}: contains /home/ubuntu", idx + 1));
            }
            if is_integration && line.contains("/run/user/1000") {
                hits.push(format!("{rel}:{}: contains /run/user/1000", idx + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "hardcoded paths in tests:\n{}",
        hits.join("\n")
    );
}

#[test]
fn cli_help_probes_do_not_write_to_home() {
    let root = repo_root();
    let roots = std::env::var_os("CARGO_TARGET_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("test-roots");
    std::fs::create_dir_all(&roots).unwrap();
    let sentinel = tempfile::Builder::new()
        .prefix("hermetic-cli-sentinel-")
        .tempdir_in(&roots)
        .unwrap();

    let ljos_bin = PathBuf::from(env!("CARGO_BIN_EXE_ljos"));
    for args in [
        &["--help"][..],
        &["memory", "integrate", "--help"][..],
        &["panel", "--help"][..],
    ] {
        let mut cmd = Command::new(&ljos_bin);
        cmd.args(args)
            .env("HOME", sentinel.path())
            .env_remove("VISSUE_ROOT")
            .env_remove("PHRONESIS_STATE_DIR")
            .stdin(Stdio::null());
        for (k, _) in std::env::vars_os() {
            if let Some(s) = k.to_str() {
                if s.starts_with("XDG_") {
                    cmd.env_remove(&k);
                }
            }
        }
        let out = cmd
            .output()
            .unwrap_or_else(|e| panic!("run {} {args:?}: {e}", ljos_bin.display()));
        assert!(
            out.status.success(),
            "{} {args:?} failed ({}):\n{}\n{}",
            ljos_bin.display(),
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        let mut leaked = Vec::new();
        collect_entries(sentinel.path(), sentinel.path(), &mut leaked);
        assert!(
            leaked.is_empty(),
            "{} {args:?} touched sentinel HOME ({}): {leaked:?}",
            ljos_bin.display(),
            sentinel.path().display()
        );
    }
}

#[test]
fn ci_test_hermetic_catches_real_home_writes_find_errors_and_keeps_exit() {
    let root = repo_root();
    let script = root.join("scripts/ci-test-hermetic.sh");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
        .join("test-roots");
    std::fs::create_dir_all(&target).unwrap();

    let run_with_home = |real_home: &Path, sh_cmd: &str| {
        Command::new("sh")
            .arg(&script)
            .args(["sh", "-c", sh_cmd])
            .current_dir(&root)
            .env("HOME", real_home)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };

    // 1. Clean command exits 0.
    let h_clean = tempfile::Builder::new()
        .prefix("ci-herm-clean-")
        .tempdir_in(&target)
        .unwrap();
    let out = run_with_home(h_clean.path(), "true");
    assert!(
        out.status.success(),
        "clean run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 2. Writes to .cache/ljos, Library/Application Support/ljos, and ~/ljos-leak all fail.
    for (rel, needle) in [
        (".cache/ljos/jev/x", ".cache/ljos"),
        (
            "Library/Application Support/ljos/x",
            "Library/Application Support/ljos",
        ),
        ("ljos-leak", "ljos-leak"),
    ] {
        let h = tempfile::Builder::new()
            .prefix("ci-herm-leak-")
            .tempdir_in(&target)
            .unwrap();
        let target_file = h.path().join(rel);
        let cmd = format!(
            "mkdir -p '{}' && : > '{}'",
            target_file.parent().unwrap().display(),
            target_file.display()
        );
        let out = run_with_home(h.path(), &cmd);
        assert_eq!(out.status.code(), Some(1), "expected exit 1 for {rel}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(needle), "stderr missing {needle}: {err}");
    }

    // 3. Pre-existing .cache/ljos fails before running the command.
    let h_pre = tempfile::Builder::new()
        .prefix("ci-herm-pre-")
        .tempdir_in(&target)
        .unwrap();
    std::fs::create_dir_all(h_pre.path().join(".cache/ljos")).unwrap();
    let marker = h_pre.path().join("ran");
    let out = run_with_home(h_pre.path(), &format!(": > '{}'", marker.display()));
    assert_eq!(out.status.code(), Some(1));
    assert!(!marker.exists(), "command should not run on dirty home");

    // 4. Mode-000 dir in suite HOME fails find, still scans real HOME, and reports both.
    let h_chmod = tempfile::Builder::new()
        .prefix("ci-herm-chmod-")
        .tempdir_in(&target)
        .unwrap();
    let cache_leak = h_chmod.path().join(".cache/ljos/from-chmod-test");
    let cmd = format!(
        "mkdir -p \"$HOME/locked\" && chmod 000 \"$HOME/locked\" && mkdir -p '{}' && : > '{}'",
        cache_leak.parent().unwrap().display(),
        cache_leak.display()
    );
    let out = run_with_home(h_chmod.path(), &cmd);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("scan of HOME") && err.contains("find exited"),
        "expected suite_home find error in stderr: {err}"
    );
    assert!(
        err.contains(".cache/ljos/from-chmod-test"),
        "mode-000 suite_home skipped real_home scan: {err}"
    );

    // 5. Command exit 9 plus two leaks prints every leak and preserves exit 9.
    let h_multi = tempfile::Builder::new()
        .prefix("ci-herm-multi-")
        .tempdir_in(&target)
        .unwrap();
    let leak_a = h_multi.path().join(".config/ljos/a");
    let leak_b = h_multi.path().join(".cache/ljos/b");
    let leak_c = h_multi.path().join("ljos-leak");
    let cmd = format!(
        "mkdir -p '{}' '{}' && : > '{}' && : > '{}' && : > '{}' && exit 9",
        leak_a.parent().unwrap().display(),
        leak_b.parent().unwrap().display(),
        leak_a.display(),
        leak_b.display(),
        leak_c.display()
    );
    let out = run_with_home(h_multi.path(), &cmd);
    assert_eq!(out.status.code(), Some(9));
    let err = String::from_utf8_lossy(&out.stderr);
    for needle in [".config/ljos/a", ".cache/ljos/b", "ljos-leak"] {
        assert!(err.contains(needle), "stderr missing {needle}: {err}");
    }

    // 6. Appending to ~/.ljos-config, same-length overwriting ~/.ljos/known_hosts,
    // and deleting a pre-existing file in ~/.ljos all fail.
    let h_mut = tempfile::Builder::new()
        .prefix("ci-herm-mut-")
        .tempdir_in(&target)
        .unwrap();
    let ljos_config = h_mut.path().join(".ljos-config");
    let ljos_dir = h_mut.path().join(".ljos");
    let known_hosts = ljos_dir.join("known_hosts");
    let to_delete = ljos_dir.join("id_test");
    std::fs::create_dir_all(&ljos_dir).unwrap();
    std::fs::write(&ljos_config, "[user]\n\tname = Original\n").unwrap();
    std::fs::write(&known_hosts, "host-a ssh-ed25519 AAAA\n").unwrap();
    std::fs::write(&to_delete, "ephemeral\n").unwrap();

    // 6a. Append to ~/.ljos-config
    let out_append = run_with_home(
        h_mut.path(),
        &format!("printf 'email = x@y\\n' >> '{}'", ljos_config.display()),
    );
    assert_eq!(out_append.status.code(), Some(1));
    let err_append = String::from_utf8_lossy(&out_append.stderr);
    assert!(
        err_append.contains("modified") && err_append.contains(".ljos-config"),
        "expected .ljos-config modification in stderr: {err_append}"
    );

    // 6b. Same-length in-place overwrite of ~/.ljos/known_hosts
    let out_overwrite = run_with_home(
        h_mut.path(),
        &format!(
            "printf 'host-b ssh-ed25519 BBBB\\n' > '{}'",
            known_hosts.display()
        ),
    );
    assert_eq!(out_overwrite.status.code(), Some(1));
    let err_overwrite = String::from_utf8_lossy(&out_overwrite.stderr);
    assert!(
        err_overwrite.contains("modified") && err_overwrite.contains(".ljos/known_hosts"),
        "expected .ljos/known_hosts modification in stderr: {err_overwrite}"
    );

    // 6c. Deletion of a pre-existing file under ~/.ljos
    let out_delete = run_with_home(h_mut.path(), &format!("rm -f '{}'", to_delete.display()));
    assert_eq!(out_delete.status.code(), Some(1));
    let err_delete = String::from_utf8_lossy(&out_delete.stderr);
    assert!(
        err_delete.contains("deleted") && err_delete.contains(".ljos/id_test"),
        "expected .ljos/id_test deletion in stderr: {err_delete}"
    );

    // 7. Same-length overwrite with restored mtime deeper than two levels under
    // ~/.ljos-deep is caught by content hashing.
    let deep = h_mut.path().join(".ljos-deep/a/b/c/deep.txt");
    let ref_mtime = h_mut.path().join("ljos-ref-mtime");
    std::fs::create_dir_all(deep.parent().unwrap()).unwrap();
    std::fs::write(&deep, "alpha-01\n").unwrap();
    std::fs::write(&ref_mtime, "").unwrap();
    let out_deep = run_with_home(
        h_mut.path(),
        &format!(
            "touch -r '{d}' '{s}' && printf 'bravo-02\\n' > '{d}' && touch -r '{s}' '{d}'",
            d = deep.display(),
            s = ref_mtime.display(),
        ),
    );
    assert_eq!(out_deep.status.code(), Some(1));
    let err_deep = String::from_utf8_lossy(&out_deep.stderr);
    assert!(
        err_deep.contains("modified") && err_deep.contains(".ljos-deep/a/b/c/deep.txt"),
        "expected deep content-hash modification in stderr: {err_deep}"
    );

    // 8. ~/.cargo and ~/.rustup are scanned even when CARGO_HOME / RUSTUP_HOME
    // are unset, and ~/.cargo/config.toml is scanned even when CARGO_HOME=~/.cargo.
    let h_cargo = tempfile::Builder::new()
        .prefix("ci-herm-cargo-")
        .tempdir_in(&target)
        .unwrap();
    for (rel, set_cargo_home) in [
        (".cargo/config.toml", false),
        (".rustup/settings.toml", false),
        (".cargo/config.toml", true),
    ] {
        let h_sub = tempfile::Builder::new()
            .prefix("ci-herm-cargo-case-")
            .tempdir_in(h_cargo.path())
            .unwrap();
        let target_file = h_sub.path().join(rel);
        let sh_cmd = format!(
            "mkdir -p '{}' && printf 'x = 1\\n' > '{}'",
            target_file.parent().unwrap().display(),
            target_file.display()
        );
        let mut cmd = Command::new("sh");
        cmd.arg(&script)
            .args(["sh", "-c", &sh_cmd])
            .current_dir(&root)
            .env("HOME", h_sub.path())
            .env_remove("RUSTUP_HOME")
            .stdin(Stdio::null());
        if set_cargo_home {
            cmd.env("CARGO_HOME", h_sub.path().join(".cargo"));
        } else {
            cmd.env_remove("CARGO_HOME");
        }
        let out = cmd.output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "expected leak failure for {rel} (CARGO_HOME={set_cargo_home}): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(rel), "stderr missing {rel}: {err}");
    }

    // 9. Pre-existing symlinks (file, dir, dangling, and cycle) into HOME are
    // walked safely and writes through them fail. `ext` is not a scanned
    // name, so only the links reach it.
    #[cfg(unix)]
    {
        let h_sym_root = tempfile::Builder::new()
            .prefix("ci-herm-sym-")
            .tempdir_in(&target)
            .unwrap();
        let h_sym = h_sym_root.path().join("home");
        let ext = h_sym.join("ext");
        let ext_sub = ext.join("dir/sub");
        std::fs::create_dir_all(&h_sym).unwrap();
        std::fs::create_dir_all(&ext_sub).unwrap();
        let ext_file = ext.join("target.txt");
        let ext_sub_file = ext_sub.join("old.txt");
        let ext_sub_del = ext_sub.join("gone.txt");
        let ext_dangling = ext.join("later.txt");
        std::fs::write(&ext_file, "orig-1\n").unwrap();
        std::fs::write(&ext_sub_file, "orig-2\n").unwrap();
        std::fs::write(&ext_sub_del, "bye\n").unwrap();
        let sym_file = h_sym.join("ljos-sym-file");
        let sym_dir = h_sym.join("ljos-sym-dir");
        let sym_dangling = h_sym.join("ljos-sym-dangling");
        let sym_loop = h_sym.join("ljos-sym-loop");
        std::os::unix::fs::symlink(&ext_file, &sym_file).unwrap();
        std::os::unix::fs::symlink(ext.join("dir"), &sym_dir).unwrap();
        std::os::unix::fs::symlink(&ext_dangling, &sym_dangling).unwrap();
        std::os::unix::fs::symlink(&h_sym, &sym_loop).unwrap();

        // 9a. Cycle terminates cleanly on a no-op command.
        let out_noop = run_with_home(&h_sym, "true");
        assert!(
            out_noop.status.success(),
            "symlink cycle broke clean scan: {}",
            String::from_utf8_lossy(&out_noop.stderr)
        );

        // 9b. Same-length overwrite with restored mtime through file symlink fails.
        let sym_ref = h_sym_root.path().join("sym-ref");
        let out_sym_file = run_with_home(
            &h_sym,
            &format!(
                "touch -r '{f}' '{s}' && printf 'edit-1\\n' > '{f}' && touch -r '{s}' '{f}'",
                f = sym_file.display(),
                s = sym_ref.display()
            ),
        );
        assert_eq!(out_sym_file.status.code(), Some(1));
        let err_sym_file = String::from_utf8_lossy(&out_sym_file.stderr);
        assert!(
            err_sym_file.contains("ljos-sym-file"),
            "expected ljos-sym-file overwrite in stderr: {err_sym_file}"
        );

        // 9c. Creating a file inside a pre-existing symlinked directory fails.
        let out_sym_dir_add = run_with_home(
            &h_sym,
            &format!(": > '{}'", sym_dir.join("sub/new.txt").display()),
        );
        assert_eq!(out_sym_dir_add.status.code(), Some(1));
        let err_sym_dir_add = String::from_utf8_lossy(&out_sym_dir_add.stderr);
        assert!(
            err_sym_dir_add.contains("ljos-sym-dir/sub/new.txt"),
            "expected ljos-sym-dir/sub/new.txt in stderr: {err_sym_dir_add}"
        );

        // 9d. Modifying a file inside a pre-existing symlinked directory fails.
        let out_sym_dir_mod = run_with_home(
            &h_sym,
            &format!(
                "touch -r '{f}' '{s}' && printf 'edit-2\\n' > '{f}' && touch -r '{s}' '{f}'",
                f = sym_dir.join("sub/old.txt").display(),
                s = sym_ref.display()
            ),
        );
        assert_eq!(out_sym_dir_mod.status.code(), Some(1));
        let err_sym_dir_mod = String::from_utf8_lossy(&out_sym_dir_mod.stderr);
        assert!(
            err_sym_dir_mod.contains("ljos-sym-dir/sub/old.txt"),
            "expected ljos-sym-dir/sub/old.txt in stderr: {err_sym_dir_mod}"
        );

        // 9e. Deleting a file inside a pre-existing symlinked directory fails.
        let out_sym_dir_del = run_with_home(
            &h_sym,
            &format!("rm -f '{}'", sym_dir.join("sub/gone.txt").display()),
        );
        assert_eq!(out_sym_dir_del.status.code(), Some(1));
        let err_sym_dir_del = String::from_utf8_lossy(&out_sym_dir_del.stderr);
        assert!(
            err_sym_dir_del.contains("ljos-sym-dir/sub/gone.txt"),
            "expected ljos-sym-dir/sub/gone.txt in stderr: {err_sym_dir_del}"
        );

        // 9f. Creating the target of a pre-existing dangling symlink fails.
        let out_dangling = run_with_home(&h_sym, &format!(": > '{}'", ext_dangling.display()));
        assert_eq!(out_dangling.status.code(), Some(1));
        let err_dangling = String::from_utf8_lossy(&out_dangling.stderr);
        assert!(
            err_dangling.contains("ljos-sym-dangling"),
            "expected ljos-sym-dangling target creation in stderr: {err_dangling}"
        );
    }

    // 10. TMPDIR and XDG_RUNTIME_DIR point to clean scratch directories,
    // are checked for scan errors, and are removed when the run ends.
    let out_scratch = run_with_home(
        h_clean.path(),
        "printf '%s\\n%s\\n' \"$TMPDIR\" \"$XDG_RUNTIME_DIR\" && test -d \"$TMPDIR\" && test -d \"$XDG_RUNTIME_DIR\"",
    );
    assert!(
        out_scratch.status.success(),
        "scratch env check failed: {}",
        String::from_utf8_lossy(&out_scratch.stderr)
    );
    let scratch_lines: Vec<&str> = std::str::from_utf8(&out_scratch.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(scratch_lines.len(), 2, "{scratch_lines:?}");
    let tmp_path = Path::new(scratch_lines[0]);
    let run_path = Path::new(scratch_lines[1]);
    assert!(
        tmp_path.to_string_lossy().contains("ljos-hermetic-tmp."),
        "TMPDIR not isolated to dedicated scratch dir: {}",
        tmp_path.display()
    );
    assert!(
        !run_path.starts_with("/tmp") && run_path.to_string_lossy().contains("hermetic-scan."),
        "XDG_RUNTIME_DIR not isolated under target: {}",
        run_path.display()
    );
    assert!(!tmp_path.exists(), "TMPDIR scratch was not cleaned up");
    assert!(
        !run_path.exists(),
        "XDG_RUNTIME_DIR scratch was not cleaned up"
    );

    for (var, cmd) in [
        (
            "TMPDIR",
            "mkdir \"$TMPDIR/locked\" && chmod 000 \"$TMPDIR/locked\"",
        ),
        (
            "XDG_RUNTIME_DIR",
            "mkdir \"$XDG_RUNTIME_DIR/locked\" && chmod 000 \"$XDG_RUNTIME_DIR/locked\"",
        ),
    ] {
        let out_err = run_with_home(h_clean.path(), cmd);
        assert_eq!(
            out_err.status.code(),
            Some(1),
            "expected scan failure for {var}"
        );
        let err = String::from_utf8_lossy(&out_err.stderr);
        assert!(
            err.contains(&format!("scan of {var}")),
            "stderr missing scan error for {var}: {err}"
        );
    }

    // 11. Writing under /tmp outside scratch TMPDIR (direct write or
    // `env -u TMPDIR mktemp`) fails, while writing inside $TMPDIR passes.
    if Path::new("/tmp").is_dir() {
        struct TmpCleanup(Vec<PathBuf>);
        impl Drop for TmpCleanup {
            fn drop(&mut self) {
                for p in &self.0 {
                    let _ = std::fs::remove_dir_all(p);
                    let _ = std::fs::remove_file(p);
                }
            }
        }

        let mut cleanup = TmpCleanup(Vec::new());
        let direct_name = format!("ljos-hermetic-leak-test-{}", std::process::id());
        let direct_path = Path::new("/tmp").join(&direct_name);
        cleanup.0.push(direct_path.clone());

        let out_direct = run_with_home(h_clean.path(), &format!(": > '{}'", direct_path.display()));
        let err_direct = String::from_utf8_lossy(&out_direct.stderr);
        assert_eq!(
            out_direct.status.code(),
            Some(1),
            "expected failure for direct write under /tmp: {err_direct}"
        );
        assert!(
            err_direct.contains(&direct_name),
            "stderr missing leaked /tmp entry {direct_name}: {err_direct}"
        );

        let out_mktemp = run_with_home(h_clean.path(), "env -u TMPDIR mktemp");
        let mktemp_created = String::from_utf8_lossy(&out_mktemp.stdout)
            .trim()
            .to_string();
        if !mktemp_created.is_empty() {
            cleanup.0.push(PathBuf::from(&mktemp_created));
        }
        let err_mktemp = String::from_utf8_lossy(&out_mktemp.stderr);
        assert_eq!(
            out_mktemp.status.code(),
            Some(1),
            "expected failure for `env -u TMPDIR mktemp` ({mktemp_created}): {err_mktemp}"
        );
        assert!(
            !mktemp_created.is_empty() && err_mktemp.contains(&mktemp_created),
            "stderr missing mktemp path {mktemp_created}: {err_mktemp}"
        );

        let out_ok = run_with_home(h_clean.path(), ": > \"$TMPDIR/ok\"");
        assert!(
            out_ok.status.success(),
            "writing inside scratch TMPDIR should pass: {}",
            String::from_utf8_lossy(&out_ok.stderr)
        );
    }

    // 12. When CARGO_HOME and RUSTUP_HOME are unset in the outer env, the
    // wrapper passes real_home defaults through (and CI workflows pass them).
    let out_cargo_env = Command::new("sh")
        .arg(&script)
        .args([
            "sh",
            "-c",
            "printf '%s\\n%s\\n' \"${CARGO_HOME:-unset}\" \"${RUSTUP_HOME:-unset}\"",
        ])
        .current_dir(&root)
        .env("HOME", h_clean.path())
        .env_remove("CARGO_HOME")
        .env_remove("RUSTUP_HOME")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out_cargo_env.status.success(),
        "unset CARGO_HOME/RUSTUP_HOME probe failed: {}",
        String::from_utf8_lossy(&out_cargo_env.stderr)
    );
    let cargo_lines: Vec<&str> = std::str::from_utf8(&out_cargo_env.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(
        cargo_lines,
        vec![
            h_clean.path().join(".cargo").to_string_lossy().into_owned(),
            h_clean
                .path()
                .join(".rustup")
                .to_string_lossy()
                .into_owned(),
        ],
        "wrapper must pass default CARGO_HOME and RUSTUP_HOME through"
    );

    let ci_yml = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
    let mut logical = String::new();
    let mut start_line = 1usize;
    for (idx, line) in ci_yml.lines().enumerate() {
        if logical.is_empty() {
            start_line = idx + 1;
        } else {
            logical.push(' ');
        }
        let trimmed = line.trim_end();
        if let Some(prefix) = trimmed.strip_suffix('\\') {
            logical.push_str(prefix.trim());
            continue;
        }
        logical.push_str(trimmed.trim());
        if logical.contains("scripts/ci-test-hermetic.sh") {
            assert!(
                logical.contains("CARGO_HOME") && logical.contains("RUSTUP_HOME"),
                ".github/workflows/ci.yml:{start_line} wraps cargo without CARGO_HOME/RUSTUP_HOME: {logical}"
            );
        }
        logical.clear();
    }

    // 13. Concurrent foreign writes to real HOME or /tmp (and pre-existing
    // ~/.cache/ljos/sibling-bin) do not fail the wrapper. Sibling tool HOME
    // leaks still fail.
    let h_foreign = tempfile::Builder::new()
        .prefix("ci-herm-foreign-")
        .tempdir_in(&target)
        .unwrap();
    std::fs::create_dir_all(h_foreign.path().join(".cursor/skills-cursor")).unwrap();
    std::fs::create_dir_all(h_foreign.path().join(".config/gh")).unwrap();
    std::fs::create_dir_all(h_foreign.path().join(".cache/ljos/sibling-bin")).unwrap();
    std::fs::write(h_foreign.path().join(".gitconfig"), "[user]\n\tname = A\n").unwrap();
    std::fs::write(
        h_foreign.path().join(".config/gh/hosts.yml"),
        "github.com:\n",
    )
    .unwrap();
    std::fs::write(
        h_foreign.path().join(".cache/ljos/sibling-bin/vissue"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    let out_foreign_home = run_with_home(
        h_foreign.path(),
        &format!(
            "printf 'email = x@y\\n' >> '{h}/.gitconfig' && \
             : > '{h}/.cursor/skills-cursor/new' && \
             printf '  user: a\\n' >> '{h}/.config/gh/hosts.yml' && \
             : > '{h}/unrelated.txt'",
            h = h_foreign.path().display()
        ),
    );
    assert!(
        out_foreign_home.status.success(),
        "foreign HOME write should pass: {}",
        String::from_utf8_lossy(&out_foreign_home.stderr)
    );

    let out_sibling_leak = run_with_home(
        h_foreign.path(),
        &format!(
            "mkdir -p '{h}/.local/share/vissue' && : > '{h}/.local/share/vissue/leak'",
            h = h_foreign.path().display()
        ),
    );
    assert_eq!(out_sibling_leak.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out_sibling_leak.stderr).contains(".local/share/vissue/leak"),
        "expected sibling-tool HOME leak in stderr"
    );

    if Path::new("/tmp").is_dir() {
        struct ForeignTmpGuard(PathBuf);
        impl Drop for ForeignTmpGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let foreign_tmp = PathBuf::from(format!("/tmp/r8-foreign-{}", std::process::id()));
        let _guard = ForeignTmpGuard(foreign_tmp.clone());
        let out_foreign_tmp =
            run_with_home(h_clean.path(), &format!(": > '{}'", foreign_tmp.display()));
        assert!(
            out_foreign_tmp.status.success(),
            "foreign /tmp write should pass: {}",
            String::from_utf8_lossy(&out_foreign_tmp.stderr)
        );

        // 14a. Same-name replace in /tmp fails (inode/mtime changed).
        let replace_path =
            PathBuf::from(format!("/tmp/ljos-hermetic-replace-{}", std::process::id()));
        let _replace_guard = ForeignTmpGuard(replace_path.clone());
        std::fs::write(&replace_path, "orig\n").unwrap();
        let out_replace = run_with_home(
            h_clean.path(),
            &format!(
                "rm -f '{p}' && printf 'new\\n' > '{p}'",
                p = replace_path.display()
            ),
        );
        assert_eq!(
            out_replace.status.code(),
            Some(1),
            "same-name replace in /tmp must fail"
        );
        let err_replace = String::from_utf8_lossy(&out_replace.stderr);
        assert!(
            err_replace.contains("ljos-hermetic-replace-"),
            "stderr missing replaced /tmp path: {err_replace}"
        );
    }

    // 14b & 14c. Nested file under existing ljos-$UID and symlink at ljos-$UID fail.
    #[cfg(unix)]
    {
        // SAFETY: geteuid is always safe to call and has no preconditions.
        let uid = unsafe { libc::geteuid() };
        let run_with_tmp_root = |tmp_root: &Path, sh_cmd: &str| {
            Command::new("sh")
                .arg(&script)
                .args(["sh", "-c", sh_cmd])
                .current_dir(&root)
                .env("HOME", h_clean.path())
                .env("LJOS_HERMETIC_TMP_ROOT", tmp_root)
                .stdin(Stdio::null())
                .output()
                .unwrap()
        };

        // 14b. Nested file under an existing ljos-$UID directory fails.
        let tmp_nested = tempfile::Builder::new()
            .prefix("ci-herm-tmp-nested-")
            .tempdir_in(&target)
            .unwrap();
        let rt_sub = tmp_nested.path().join(format!("ljos-{uid}/existing-sub"));
        std::fs::create_dir_all(&rt_sub).unwrap();
        let nested_file = rt_sub.join("nested.txt");
        let out_nested = run_with_tmp_root(
            tmp_nested.path(),
            &format!(": > '{}'", nested_file.display()),
        );
        assert_eq!(
            out_nested.status.code(),
            Some(1),
            "nested write under existing ljos-$UID must fail"
        );
        let err_nested = String::from_utf8_lossy(&out_nested.stderr);
        assert!(
            err_nested.contains("nested.txt"),
            "stderr missing nested file under ljos-$UID: {err_nested}"
        );

        // 14c-1. Creating a symlink at ljos-$UID fails.
        let tmp_sym_new = tempfile::Builder::new()
            .prefix("ci-herm-tmp-sym-new-")
            .tempdir_in(&target)
            .unwrap();
        let ext_new = tmp_sym_new.path().join("ext");
        std::fs::create_dir_all(&ext_new).unwrap();
        let rt_new = tmp_sym_new.path().join(format!("ljos-{uid}"));
        let out_sym_new = run_with_tmp_root(
            tmp_sym_new.path(),
            &format!("ln -s '{}' '{}'", ext_new.display(), rt_new.display()),
        );
        assert_eq!(
            out_sym_new.status.code(),
            Some(1),
            "creating symlink at ljos-$UID must fail"
        );
        let err_sym_new = String::from_utf8_lossy(&out_sym_new.stderr);
        assert!(
            err_sym_new.contains(&format!("ljos-{uid}")),
            "stderr missing symlink at ljos-$UID: {err_sym_new}"
        );

        // 14c-2. Pre-existing symlink at ljos-$UID passes when untouched, and
        // writing through it fails.
        let tmp_sym_pre = tempfile::Builder::new()
            .prefix("ci-herm-tmp-sym-pre-")
            .tempdir_in(&target)
            .unwrap();
        let ext_pre = tmp_sym_pre.path().join("ext");
        std::fs::create_dir_all(&ext_pre).unwrap();
        let rt_pre = tmp_sym_pre.path().join(format!("ljos-{uid}"));
        std::os::unix::fs::symlink(&ext_pre, &rt_pre).unwrap();

        let out_sym_clean = run_with_tmp_root(tmp_sym_pre.path(), "true");
        assert!(
            out_sym_clean.status.success(),
            "untouched pre-existing ljos-$UID symlink should pass: {}",
            String::from_utf8_lossy(&out_sym_clean.stderr)
        );

        let out_sym_write = run_with_tmp_root(
            tmp_sym_pre.path(),
            &format!(": > '{}'", rt_pre.join("via-symlink").display()),
        );
        assert_eq!(
            out_sym_write.status.code(),
            Some(1),
            "writing through pre-existing ljos-$UID symlink must fail"
        );
        let err_sym_write = String::from_utf8_lossy(&out_sym_write.stderr);
        assert!(
            err_sym_write.contains("via-symlink"),
            "stderr missing write through ljos-$UID symlink: {err_sym_write}"
        );
    }
}

/// A scratch directory under the target dir for one wrapper case.
fn hermetic_case_dir(prefix: &str) -> tempfile::TempDir {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
        .join("test-roots");
    std::fs::create_dir_all(&target).unwrap();
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(&target)
        .unwrap()
}

/// Runs `sh -c sh_cmd` under the wrapper with `home` as the real HOME and
/// `tmp_root` in place of `/tmp`.
fn run_hermetic(home: &Path, tmp_root: &Path, sh_cmd: &str) -> std::process::Output {
    let root = repo_root();
    Command::new("sh")
        .arg(root.join("scripts/ci-test-hermetic.sh"))
        .args(["sh", "-c", sh_cmd])
        .current_dir(&root)
        .env("HOME", home)
        .env("LJOS_HERMETIC_TMP_ROOT", tmp_root)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn ci_test_hermetic_bounds_symlink_walks_to_scan_root() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    struct Unlock(PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    // SAFETY: geteuid is always safe to call and has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let case = hermetic_case_dir("ci-herm-bound-");
    let home = case.path().join("home");
    let tmp = case.path().join("tmp");
    let outside = case.path().join("outside");
    let locked = case.path().join("locked");
    for d in [&home, &tmp, &outside, &locked.join("inner")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let _unlock = Unlock(locked.clone());
    let home_link = home.join(".ljos");
    let rt_link = tmp.join(format!("ljos-{uid}"));

    // A. Links out of the root to an unreadable directory leave a clean run
    // passing.
    symlink(&locked, &home_link).unwrap();
    symlink(&locked, &rt_link).unwrap();
    let out_locked = run_hermetic(&home, &tmp, "true");
    assert!(
        out_locked.status.success(),
        "links to an unreadable outside dir failed a clean run: {}",
        String::from_utf8_lossy(&out_locked.stderr)
    );

    // B. Foreign writes beyond a link to an outside dir or to the root itself
    // are not reported.
    std::fs::remove_file(&home_link).unwrap();
    std::fs::remove_file(&rt_link).unwrap();
    symlink(&outside, &home_link).unwrap();
    symlink(&tmp, &rt_link).unwrap();
    let out_foreign = run_hermetic(
        &home,
        &tmp,
        &format!(
            ": > '{}' && : > '{}'",
            outside.join("foreign.txt").display(),
            tmp.join("foreign-name.txt").display()
        ),
    );
    assert!(
        out_foreign.status.success(),
        "foreign writes beyond out-of-root links were reported: {}",
        String::from_utf8_lossy(&out_foreign.stderr)
    );

    // C. Retargeting a recorded link still fails.
    let out_retarget = run_hermetic(
        &home,
        &tmp,
        &format!(
            "ln -sfn '{}' '{}' && ln -sfn '{}' '{}'",
            locked.display(),
            home_link.display(),
            outside.display(),
            rt_link.display()
        ),
    );
    assert_eq!(
        out_retarget.status.code(),
        Some(1),
        "retargeted links must fail"
    );
    let err_retarget = String::from_utf8_lossy(&out_retarget.stderr);
    for needle in ["home/.ljos (", &format!("tmp/ljos-{uid} (")] {
        assert!(
            err_retarget.contains(needle),
            "stderr missing {needle}: {err_retarget}"
        );
    }
}
