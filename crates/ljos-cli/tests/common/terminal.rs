//! A program run on a fresh pseudo-terminal, in a process shape a test
//! names, so the person-at-the-terminal check sees what a terminal gives.

/// Runs `argv` on a fresh pty in the shape `mode` names: `terminal`, a
/// parent renamed (`renamed:NAME`), or below a process left by a double
/// fork (`double-fork`) or reparented without one (`reparented`). Prints
/// what the program printed and exits with its status.
const SHAPE_PY: &str = r#"import ctypes, os, pty, sys, time
mode, argv = sys.argv[1], sys.argv[2:]

def run():
    pid, fd = pty.fork()
    if pid == 0:
        os.execv(argv[0], argv)
    out = []
    while True:
        try:
            data = os.read(fd, 4096)
        except OSError:
            break
        if not data:
            break
        out.append(data)
    _, status = os.waitpid(pid, 0)
    return b"".join(out), os.waitstatus_to_exitcode(status)

def orphan(setsid):
    r, w = os.pipe()
    if os.fork() == 0:
        os.close(r)
        if setsid:
            os.setsid()
        parent = os.getpid()
        if os.fork() != 0:
            os._exit(0)
        while os.getppid() == parent:
            time.sleep(0.01)
        said, code = run()
        os.write(w, bytes([code & 0xff]) + said)
        os._exit(0)
    os.close(w)
    os.wait()
    got = b""
    while True:
        chunk = os.read(r, 65536)
        if not chunk:
            break
        got += chunk
    return got[1:], (got[0] if got else 99)

if mode.startswith("renamed:"):
    libc = ctypes.CDLL(None)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_char_p]
    libc.prctl.restype = ctypes.c_int
    if libc.prctl(15, mode.split(":", 1)[1].encode()) != 0:
        raise SystemExit(91)
    said, code = run()
elif mode == "double-fork":
    said, code = orphan(True)
elif mode == "reparented":
    said, code = orphan(False)
else:
    # The person at a terminal has a terminal emulator holding the pty.
    # python's own name is not one, so the default shape takes one.
    libc = ctypes.CDLL(None)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_char_p]
    libc.prctl.restype = ctypes.c_int
    if libc.prctl(15, b"gnome-terminal-") != 0:
        raise SystemExit(91)
    said, code = run()
sys.stdout.buffer.write(said)
raise SystemExit(code)
"#;

/// The interpreter that runs the shape.
pub const PYTHON: &str = "/usr/bin/python3";

/// The arguments [`PYTHON`] takes to run `bin ARGS` in the shape `mode`.
#[must_use]
pub fn shape<'a>(mode: &'a str, bin: &'a str, args: &[&'a str]) -> Vec<&'a str> {
    let mut all = vec!["-c", SHAPE_PY, mode, bin];
    all.extend_from_slice(args);
    all
}
