//! Verify Rust, the guard shim, and scripts/install.sh agree on temporary HOME and TMPDIR classification.

ljos_test_boot::harness!();

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "common/runner.rs"]
mod runner;

fn write_exe(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn install_sh_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh")
}

fn make_stub_progs(dir: &Path) {
    for name in [
        "ljos",
        "ljos-mcp",
        "packset",
        "packsetd",
        "packset-mcp",
        "vissue",
        "vissue-mcp",
        "deedar",
        "claimdag",
        "ljos-policyd",
        "ljos-consensus",
    ] {
        write_exe(&dir.join(name), "#!/bin/sh\necho \"0.45.3\"\n");
    }
}

fn setup_fake_bin(dir: &Path, pw_home: &Path, real_rm_dir: &Path) {
    let getent = format!(
        "#!/bin/sh\nprintf 'person:x:1000:1000::%s:/bin/sh\\n' '{}'\n",
        pw_home.display()
    );
    write_exe(&dir.join("getent"), &getent);
    write_exe(
        &dir.join("ljos-policyd"),
        "#!/bin/sh\nprintf 'allow\\twarn\\tRULE\\tGUARDED\\n'\n",
    );
    write_exe(&dir.join("ljos"), "#!/bin/sh\nexit 0\n");
    write_exe(&real_rm_dir.join("rm"), "#!/bin/sh\nexit 0\n");
}

struct Row<'a> {
    label: &'a str,
    home: &'a Path,
    tmpdir: Option<&'a str>,
    cwd: &'a Path,
    xdg_state: Option<&'a Path>,
    expected_temp: bool,
    mktemp_ok: bool,
}

fn skip_root_readonly_mktemp_failure(euid: u32, label: &str) -> bool {
    euid == 0 && matches!(label, "tmpdir=/home" | "tmpdir=/")
}

#[test]
fn root_euid_skips_only_readonly_system_tmpdir_failure_rows() {
    assert!(skip_root_readonly_mktemp_failure(0, "tmpdir=/home"));
    assert!(skip_root_readonly_mktemp_failure(0, "tmpdir=/"));
    assert!(!skip_root_readonly_mktemp_failure(
        0,
        "missing_path_under_session"
    ));
    assert!(!skip_root_readonly_mktemp_failure(1000, "tmpdir=/home"));
    assert!(!skip_root_readonly_mktemp_failure(1000, "tmpdir=/"));
}

#[test]
fn shim_install_and_rust_agree_on_tmpdir_table() {
    let installed = runner::install_dir();
    let pw = installed.passwd_home().to_path_buf();
    let non_temp_parent = pw.parent().unwrap().to_path_buf();
    let pw_work = pw.join("work");
    let pw_work_h = pw_work.join("h");
    let other_home = non_temp_parent.join("other/h");
    let session_a = non_temp_parent.join("session-a");
    let session_a_u = session_a.join("u");
    let session_b = non_temp_parent.join("session-b");
    let session_b_sub = session_b.join("sub");
    let session_b_u = session_b.join("u");
    let rv3 = non_temp_parent.join("rv3");
    let rv3_u = rv3.join("u");
    let var_t_x = non_temp_parent.join("var/folders/ab/cdef1234/T/x");
    let var_t_x_u = var_t_x.join("u");
    let tmp_base = tempfile::tempdir().unwrap();
    let tmp_x = tmp_base.path().join("x");
    let tmp_x_u = tmp_x.join("u");
    for d in [
        &pw_work_h,
        &other_home,
        &session_a_u,
        &session_b_sub,
        &session_b_u,
        &rv3_u,
        &var_t_x_u,
        &tmp_x_u,
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    let sl_to_session_a = non_temp_parent.join("sl-to-session-a");
    let _ = std::fs::remove_file(&sl_to_session_a);
    std::os::unix::fs::symlink(&session_a, &sl_to_session_a).unwrap();
    let rv3_sl = rv3.join("sl");
    let _ = std::fs::remove_file(&rv3_sl);
    std::os::unix::fs::symlink(&session_b_sub, &rv3_sl).unwrap();

    let shim_dir = tmp_base.path().join("shim");
    let shim_rm = shim_dir.join("rm");
    write_exe(&shim_rm, ljos_cli::GUARD_SHIM);

    let fake_bin = tmp_base.path().join("fake-bin");
    let real_rm_dir = tmp_base.path().join("real-rm");
    setup_fake_bin(&fake_bin, &pw, &real_rm_dir);

    let stub_progs = tmp_base.path().join("stub-progs");
    make_stub_progs(&stub_progs);

    let sys_path = "/usr/bin:/bin";
    let shim_path = format!(
        "{}:{}:{}:{}",
        shim_dir.display(),
        fake_bin.display(),
        real_rm_dir.display(),
        sys_path
    );
    let install_path = format!("{}:{}", fake_bin.display(), sys_path);

    let rv3_sl_dotdot = rv3_sl.join("..");
    let session_a_missing = session_a.join("missing");
    let session_a_missing_dotdot = session_a_missing.join("..");
    let session_a_missing_up2 = session_a_missing.join("../..");
    let session_a_double_slash = format!("{}//", session_a.display());
    let session_a_missing_user = session_a_missing.join("user");
    let session_a_no_user = session_a.join("no-user");
    let session_a_no_user_xdg = session_a.join("no-user-xdg");
    let session_a_no_user_pw = session_a.join("no-user-pw");
    let session_a_no_user_pw_xdg = session_a.join("no-user-pw-xdg");
    let session_a_no_user_dotdot = session_a.join("no-user-dotdot");
    let tmp_x_missing_user = tmp_x.join("missing-user");
    let xdg_state_sep = tmp_base.path().join("xdg-state-sep");
    std::fs::create_dir_all(&xdg_state_sep).unwrap();

    let cases: &[Row<'_>] = &[
        Row {
            label: "tmpdir=/home",
            home: &pw,
            tmpdir: Some("/home"),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: false,
        },
        Row {
            label: "tmpdir=/",
            home: &pw,
            tmpdir: Some("/"),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: false,
        },
        Row {
            label: "tmpdir=ancestor_of_pw",
            home: &pw,
            tmpdir: Some(non_temp_parent.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "tmpdir=ancestor_of_pw_other_home",
            home: &other_home,
            tmpdir: Some(non_temp_parent.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "tmpdir=pw_home",
            home: &pw,
            tmpdir: Some(pw.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "home=under_pw_tmpdir=pw_home",
            home: &pw_work,
            tmpdir: Some(pw.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "home=pw_tmpdir=tmp_x",
            home: &pw,
            tmpdir: Some(tmp_x.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "home=under_tmp_x_tmpdir=tmp_x",
            home: &tmp_x_u,
            tmpdir: Some(tmp_x.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: true,
        },
        Row {
            label: "home=pw_tmpdir=var_folders",
            home: &pw,
            tmpdir: Some(var_t_x.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "home=under_var_folders_tmpdir=var_folders",
            home: &var_t_x_u,
            tmpdir: Some(var_t_x.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: true,
        },
        Row {
            label: "home=pw_tmpdir=/tmp",
            home: &pw,
            tmpdir: Some("/tmp"),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "home=pw_tmpdir=unset",
            home: &pw,
            tmpdir: None,
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "relative_tmpdir=dot",
            home: &pw_work_h,
            tmpdir: Some("."),
            cwd: &pw_work,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "relative_tmpdir=work",
            home: &pw_work_h,
            tmpdir: Some("work"),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "symlink_tmpdir=session_a",
            home: &session_a_u,
            tmpdir: Some(sl_to_session_a.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: true,
        },
        Row {
            label: "symlink_dotdot_guards_rv3",
            home: &rv3_u,
            tmpdir: Some(rv3_sl_dotdot.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "symlink_dotdot_allows_target_parent",
            home: &session_b_u,
            tmpdir: Some(rv3_sl_dotdot.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: true,
        },
        Row {
            label: "missing_dotdot_into_session",
            home: &session_a_u,
            tmpdir: Some(session_a_missing_dotdot.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: false,
        },
        Row {
            label: "missing_dotdot_to_pw_ancestor",
            home: &session_a_u,
            tmpdir: Some(session_a_missing_up2.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: false,
        },
        Row {
            label: "trailing_slash_double",
            home: &session_a_u,
            tmpdir: Some(session_a_double_slash.as_str()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: true,
        },
        Row {
            label: "missing_path_under_session",
            home: &session_a_u,
            tmpdir: Some(session_a_missing.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: false,
        },
        Row {
            label: "missing_home_under_missing_tmpdir",
            home: &session_a_missing_user,
            tmpdir: Some(session_a_missing.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: false,
        },
        Row {
            label: "missing_home_under_existing_tmpdir",
            home: &session_a_no_user,
            tmpdir: Some(session_a.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: true,
        },
        Row {
            label: "missing_home_under_existing_tmpdir_xdg_state",
            home: &session_a_no_user_xdg,
            tmpdir: Some(session_a.to_str().unwrap()),
            cwd: &pw,
            xdg_state: Some(&xdg_state_sep),
            expected_temp: true,
            mktemp_ok: true,
        },
        Row {
            label: "missing_home_under_pw_tmpdir",
            home: &session_a_no_user_pw,
            tmpdir: Some(pw.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "missing_home_under_pw_tmpdir_xdg_state",
            home: &session_a_no_user_pw_xdg,
            tmpdir: Some(pw.to_str().unwrap()),
            cwd: &pw,
            xdg_state: Some(&xdg_state_sep),
            expected_temp: false,
            mktemp_ok: true,
        },
        Row {
            label: "missing_home_with_missing_dotdot_tmpdir",
            home: &session_a_no_user_dotdot,
            tmpdir: Some(session_a_missing_dotdot.to_str().unwrap()),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: false,
        },
        Row {
            label: "missing_home_under_tmp_tmpdir",
            home: &tmp_x_missing_user,
            tmpdir: Some("/tmp"),
            cwd: &pw,
            xdg_state: None,
            expected_temp: true,
            mktemp_ok: true,
        },
    ];

    for row in cases {
        let label = row.label;
        let home = row.home;
        let tmpdir = row.tmpdir;
        let cwd = row.cwd;
        let expected_temp = row.expected_temp;
        let missing_home = label.starts_with("missing_home_");
        if missing_home {
            assert!(
                !home.exists(),
                "{label}: test setup requires nonexistent HOME {}",
                home.display()
            );
        }

        // 1. Guard shim (run before install.sh so install.sh never pre-creates a missing HOME)
        let mut shim_cmd = Command::new(&shim_rm);
        shim_cmd
            .arg("-f")
            .arg("dummy")
            .current_dir(cwd)
            .env_clear()
            .env("HOME", home)
            .env("PATH", &shim_path)
            .stdin(Stdio::null());
        if let Some(t) = tmpdir {
            shim_cmd.env("TMPDIR", t);
        }
        let shim_out = shim_cmd.output().unwrap();
        assert!(
            shim_out.status.success(),
            "{label} shim failed: {}",
            String::from_utf8_lossy(&shim_out.stderr)
        );
        let shim_err = String::from_utf8_lossy(&shim_out.stderr);
        let shim_is_temp = !shim_err.contains("GUARDED");
        assert_eq!(
            shim_is_temp, expected_temp,
            "{label}: GUARD_SHIM temp={shim_is_temp} expected={expected_temp} stderr={shim_err:?}"
        );

        // 2. Rust CLI binary (Linux uses RunnerDir LD_PRELOAD so own_home() == pw)
        #[cfg(target_os = "linux")]
        {
            if missing_home {
                assert!(
                    !home.exists(),
                    "{label}: HOME {} must still be missing before Rust probe",
                    home.display()
                );
            }
            let rust_xdg = tmp_base.path().join("rust-xdg");
            let mut rust_cmd = Command::new(env!("CARGO_BIN_EXE_ljos"));
            installed.env(&mut rust_cmd);
            rust_cmd
                .args(["search", "tmpdir-probe"])
                .current_dir(cwd)
                .env("HOME", home)
                .env("XDG_CONFIG_HOME", rust_xdg.join("config"))
                .env("XDG_DATA_HOME", rust_xdg.join("share"))
                .env("XDG_STATE_HOME", rust_xdg.join("state"))
                .env("XDG_CACHE_HOME", rust_xdg.join("cache"))
                .env("XDG_RUNTIME_DIR", rust_xdg.join("run"))
                .env("PATH", sys_path)
                .env_remove("PACKSET_URL")
                .env_remove("PACKSET_PORT")
                .env_remove("LJOS_MEM_PORT")
                .env_remove("LJOS_OFF")
                .env_remove("LJOS_GUARD")
                .stdin(Stdio::null());
            match tmpdir {
                Some(t) => {
                    rust_cmd.env("TMPDIR", t);
                }
                None => {
                    rust_cmd.env_remove("TMPDIR");
                }
            }
            let rust_out = rust_cmd.output().unwrap();
            let rust_text = format!(
                "{}{}",
                String::from_utf8_lossy(&rust_out.stdout),
                String::from_utf8_lossy(&rust_out.stderr)
            );
            let rust_is_temp = rust_text
                .contains("memory and the command guard are off because the home is under");
            assert_eq!(
                rust_is_temp, expected_temp,
                "{label}: ljos temp={rust_is_temp} expected={expected_temp}\n{rust_text}"
            );
            if missing_home {
                assert!(
                    !home.exists(),
                    "{label}: ljos search tmpdir-probe must not create missing HOME {}",
                    home.display()
                );
            }
        }

        // 3. Real scripts/install.sh (using system mktemp, no stub)
        #[cfg(unix)]
        {
            // SAFETY: geteuid is always safe to call and has no preconditions.
            if skip_root_readonly_mktemp_failure(unsafe { libc::geteuid() }, label) {
                eprintln!(
                    "skipping {label} install.sh mktemp-failure check: euid == 0 can write to {}",
                    tmpdir.unwrap_or("")
                );
                continue;
            }
        }
        let bin_dir = tmp_base.path().join(format!("inst-{label}"));
        let mut inst_cmd = Command::new("sh");
        inst_cmd
            .arg(install_sh_path())
            .arg("--from-dir")
            .arg(&stub_progs)
            .arg("--bin-dir")
            .arg(&bin_dir)
            .args(["--no-onboard", "--no-embed"])
            .current_dir(cwd)
            .env_clear()
            .env("HOME", home)
            .env("PATH", &install_path)
            .stdin(Stdio::null());
        if let Some(t) = tmpdir {
            inst_cmd.env("TMPDIR", t);
        }
        if let Some(s) = row.xdg_state {
            inst_cmd.env("XDG_STATE_HOME", s);
        }
        let inst_out = inst_cmd.output().unwrap();
        let inst_text = format!(
            "{}{}",
            String::from_utf8_lossy(&inst_out.stdout),
            String::from_utf8_lossy(&inst_out.stderr)
        );
        if row.mktemp_ok {
            assert!(
                inst_out.status.success(),
                "{label} install.sh failed: {inst_text}"
            );
            let inst_is_temp = inst_text
                .contains("memory and the command guard are off because the home is under");
            assert_eq!(
                inst_is_temp, expected_temp,
                "{label}: install.sh temp={inst_is_temp} expected={expected_temp}\n{inst_text}"
            );
            if row.xdg_state.is_some() && missing_home {
                assert!(
                    !home.exists(),
                    "{label}: install.sh with XDG_STATE_HOME set should not need to create HOME"
                );
            }
        } else {
            assert!(
                !inst_out.status.success(),
                "{label} install.sh unexpectedly succeeded without mktemp stub:\n{inst_text}"
            );
            assert!(
                inst_text.contains("mktemp: failed to create directory"),
                "{label} install.sh should report real mktemp failure:\n{inst_text}"
            );
        }
    }
}
