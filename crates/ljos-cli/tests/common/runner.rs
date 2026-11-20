//! A stand-in for a runner: a copy of the shell, so the kernel runs it
//! from a file of the runner's name, which is how `ljos hook` knows the
//! runner spawned it.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A directory for stand-ins below `~/.local/bin` of a temporary passwd
/// home outside `/tmp`, with an `LD_PRELOAD` helper on Linux so child
/// processes read that home from `getpwuid_r` instead of the real `HOME`.
pub struct RunnerDir {
    _root: tempfile::TempDir,
    passwd_home: PathBuf,
    #[allow(dead_code)]
    bin_dir: PathBuf,
    #[cfg(target_os = "linux")]
    preload: PathBuf,
}

impl RunnerDir {
    #[allow(dead_code)]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.bin_dir
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn passwd_home(&self) -> &Path {
        &self.passwd_home
    }

    pub fn env<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        #[cfg(target_os = "linux")]
        {
            cmd.env("LD_PRELOAD", &self.preload)
                .env("LJOS_TEST_PASSWD_HOME", &self.passwd_home);
        }
        cmd
    }
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[allow(dead_code)]
pub fn non_temp_homes_root() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("test-homes");
    if !ljos_cli::bookkeep::path_is_temp(Some(&target)) && std::fs::create_dir_all(&target).is_ok()
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        }
        return std::fs::canonicalize(&target).unwrap_or(target);
    }
    if let Some(home) = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
    {
        let canon_home = std::fs::canonicalize(&home).unwrap_or(home);
        let canon_pw =
            ljos_cli::proc_tree::own_home().map(|pw| std::fs::canonicalize(&pw).unwrap_or(pw));
        let touches_pw = canon_pw
            .as_ref()
            .is_some_and(|pw| canon_home == *pw || pw.starts_with(&canon_home));
        if !touches_pw && !ljos_cli::bookkeep::path_is_temp(Some(&canon_home)) {
            let root = canon_home.join(".ljos-test-homes");
            if std::fs::create_dir_all(&root).is_ok() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755));
                }
                return std::fs::canonicalize(&root).unwrap_or(root);
            }
        }
    }
    panic!(
        "cannot place non-temp runner homes without touching real passwd home: target {} and HOME {:?} are temporary, missing, or equal to the passwd home",
        target.display(),
        std::env::var_os("HOME")
    );
}

#[cfg(target_os = "linux")]
fn passwd_preload_so() -> PathBuf {
    use std::sync::OnceLock;
    static SO: OnceLock<PathBuf> = OnceLock::new();
    SO.get_or_init(|| {
        let dir = std::env::var_os("CARGO_TARGET_DIR")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("test-preload");
        std::fs::create_dir_all(&dir).unwrap();
        let code = r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <pwd.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static const char *test_passwd_home(uid_t uid) {
    if (uid != getuid()) {
        return NULL;
    }
    const char *home = getenv("LJOS_TEST_PASSWD_HOME");
    return (home && home[0] == '/') ? home : NULL;
}

struct passwd *getpwuid(uid_t uid) {
    static struct passwd *(*real_fn)(uid_t) = NULL;
    static __thread struct passwd copy;
    static __thread char home_buf[4096];
    if (!real_fn) {
        real_fn = (struct passwd *(*)(uid_t))dlsym(RTLD_NEXT, "getpwuid");
    }
    struct passwd *res = real_fn ? real_fn(uid) : NULL;
    const char *home = test_passwd_home(uid);
    if (!res || !home) {
        return res;
    }
    copy = *res;
    size_t need = strlen(home) + 1;
    if (need <= sizeof(home_buf)) {
        memcpy(home_buf, home, need);
        copy.pw_dir = home_buf;
    }
    return &copy;
}

int getpwuid_r(uid_t uid, struct passwd *pwd, char *buf, size_t buflen, struct passwd **result) {
    static int (*real_fn)(uid_t, struct passwd *, char *, size_t, struct passwd **) = NULL;
    if (!real_fn) {
        real_fn = (int (*)(uid_t, struct passwd *, char *, size_t, struct passwd **))dlsym(
            RTLD_NEXT, "getpwuid_r");
    }
    int rc = real_fn ? real_fn(uid, pwd, buf, buflen, result) : ENOENT;
    if (rc != 0 || !result || !*result) {
        return rc;
    }
    const char *home = test_passwd_home(uid);
    if (!home) {
        return rc;
    }
    size_t need = strlen(home) + 1;
    const char *fields[5] = {pwd->pw_name, pwd->pw_passwd, pwd->pw_gecos, pwd->pw_dir, pwd->pw_shell};
    size_t used = 0;
    for (int i = 0; i < 5; i++) {
        if (fields[i] && fields[i] >= buf && fields[i] < buf + buflen) {
            size_t end = (size_t)(fields[i] - buf) + strlen(fields[i]) + 1;
            if (end > used) {
                used = end;
            }
        }
    }
    if (used + need > buflen) {
        *result = NULL;
        return ERANGE;
    }
    memcpy(buf + used, home, need);
    pwd->pw_dir = buf + used;
    return 0;
}
"#;
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in code.as_bytes() {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let so = dir.join(format!("libtest_passwd-{hash:016x}.so"));
        if so.is_file() {
            return so;
        }
        let pid = std::process::id();
        let src = dir.join(format!("test_passwd-{pid}.c"));
        let tmp = dir.join(format!("libtest_passwd-{pid}.so.tmp"));
        std::fs::write(&src, code)
        .unwrap();
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = Command::new(cc)
            .args(["-shared", "-fPIC", "-O2", "-o"])
            .arg(&tmp)
            .arg(&src)
            .arg("-ldl")
            .output()
            .expect("compile test passwd preload");
        let _ = std::fs::remove_file(&src);
        assert!(
            out.status.success(),
            "cc preload failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::rename(&tmp, &so);
        let _ = std::fs::remove_file(&tmp);
        so
    })
    .clone()
}

/// Create a temporary runner directory under `~/.local/bin` of an isolated
/// non-temp home (without touching the real passwd home) and remove it on drop.
pub fn install_dir() -> RunnerDir {
    let _guard = env_lock();
    let root = tempfile::Builder::new()
        .prefix("ljos-home-")
        .tempdir_in(non_temp_homes_root())
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755));
    }
    let passwd_home = root.path().join("u");
    let local = passwd_home.join(".local");
    let bin = local.join("bin");
    let bin_dir = bin.join(format!(".ljos-runner-{}", std::process::id()));
    std::fs::create_dir_all(&bin_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for dir in [&passwd_home, &local, &bin, &bin_dir] {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
        }
    }
    RunnerDir {
        _root: root,
        passwd_home,
        bin_dir,
        #[cfg(target_os = "linux")]
        preload: passwd_preload_so(),
    }
}

/// Copy `/bin/sh` to `path` unless it is there, and wait until it runs.
/// A test thread that forked while the copy was open holds it for writing
/// until that child execs, and an exec meanwhile fails as text file busy.
#[allow(dead_code)]
pub fn runner_at(path: &Path) {
    if path.exists() {
        return;
    }
    let tmp = path.with_extension("part");
    std::fs::copy("/bin/sh", &tmp).unwrap();
    std::fs::rename(&tmp, path).unwrap();
    for _ in 0..200 {
        match Command::new(path).args(["-c", ":"]).status() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return,
        }
    }
}
