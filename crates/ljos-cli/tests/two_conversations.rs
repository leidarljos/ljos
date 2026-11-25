//! Two conversations of one seat, one tracker.
//!
//! The outer shell already sits on `ljos-rpud`. The inner shell names a
//! different conversation id. `ljos sitting ljos-yw6t` claims that ticket
//! for the inner conversation. `ljos finish ljos-yw6t` finishes it. The
//! outer sitting stays where it was.

ljos_test_boot::harness!();

use std::path::Path;
use std::process::{Command, Output, Stdio};

const OUTER: &str = "conv-outer-aaaa-1111";
const INNER: &str = "conv-inner-bbbb-2222";
const SEAT: &str = "ljosbot";
const RPUD: &str = "ljos-rpud";
const YW6T: &str = "ljos-yw6t";

const ORG: &str = "\
#+TITLE: ljos
#+VISSUE: 1
#+CATEGORY: ljos
#+TODO: TODO STARTED BLOCKED | DONE CANCELLED

* TODO [#C] First sitting
:PROPERTIES:
:ID:         ljos-rpud
:END:

* TODO [#C] Second sitting
:PROPERTIES:
:ID:         ljos-yw6t
:END:
";

fn script(path: &Path, body: &str) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    ljos_cli::install_executable(path, body).unwrap();
}

fn text_of(out: &Output) -> String {
    format!(
        "status {:?}\nstdout:\n{}\nstderr:\n{}\n",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

struct Seat {
    dir: tempfile::TempDir,
}

impl Seat {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let bin = root.join("bin");
        let project = root.join("tracker/Software/ljos");
        for sub in ["home", "run", "state", "tmp"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(root.join("tracker/vissue.toml"), "prefix = \"Software\"\n").unwrap();
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(project.join("issues.org"), ORG).unwrap();
        script(&bin.join("deedar"), "#!/bin/sh\necho head\nexit 0\n");
        script(
            &bin.join("claimdag"),
            &format!(
                r#"#!/usr/bin/env python3
import json, os, sys
path = "{}/claims.json"
os.makedirs(os.path.dirname(path), exist_ok=True)
data = json.load(open(path)) if os.path.exists(path) else {{}}
args = sys.argv[1:]
def flag(name):
    if name in args:
        i = args.index(name)
        return args[i + 1] if i + 1 < len(args) else ""
    return ""
cmd = args[0] if args else ""
if cmd == "list":
    print("[]")
    sys.exit(0)
if cmd == "upsert":
    sys.exit(0)
if cmd == "claim":
    node, who = args[1], flag("--assignee")
    row = data.get(node, {{"gen": 1}})
    if row.get("status") == "claimed" and row.get("assignee") not in ("", who):
        sys.stderr.write("claim: status claimed\n")
        sys.exit(1)
    row["status"] = "claimed"
    row["assignee"] = who
    data[node] = row
    json.dump(data, open(path, "w"))
    print("claimed")
    sys.exit(0)
if cmd == "get":
    row = data.get(args[1])
    if not row:
        sys.stderr.write("not found\n")
        sys.exit(1)
    print(f"{{args[1]}} {{row.get('status','todo')}} gen={{row.get('gen',1)}} assignee={{row.get('assignee','')}}")
    sys.exit(0)
if cmd in ("renew", "release", "complete", "reopen"):
    print(cmd)
    sys.exit(0)
sys.exit(0)
"#,
                root.join("state").display()
            ),
        );
        script(
            &bin.join("vissue"),
            r#"#!/usr/bin/env python3
import json, os, re, sys
root = os.environ["VISSUE_ROOT"]
args = sys.argv[1:]
who = os.environ.get("VISSUE_AGENT", "")
def boards():
    found = []
    for dirpath, _, files in os.walk(root):
        if "issues.org" in files:
            found.append(os.path.join(dirpath, "issues.org"))
    return found
def find(issue):
    for path in boards():
        text = open(path).read()
        if re.search(rf":ID:\s+{re.escape(issue)}\s*$", text, re.M):
            return path, text
    return None, None
cmd = args[0] if args else ""
if cmd == "identity":
    print("vissue test")
    print("root=" + root)
    print("prefix=Software")
    sys.exit(0)
if cmd == "show":
    issue = args[1]
    path, text = find(issue)
    if not path:
        sys.stderr.write(f"issue {issue} not found\n")
        sys.exit(1)
    hm = re.search(rf"\* (\w+)(?: \[#[A-Z]\])? ([^\n]+)\n:PROPERTIES:\n:ID:\s+{re.escape(issue)}\b", text)
    state, title = ("TODO", issue)
    if hm:
        state, title = hm.group(1), hm.group(2).strip()
    block = re.search(rf":ID:\s+{re.escape(issue)}\n(.*?)(?::END:)", text, re.S)
    props = block.group(1) if block else ""
    claimed = ""
    cm = re.search(r":CLAIMED_BY:\s+(\S+)", props)
    if cm:
        claimed = cm.group(1)
    if "--json" in args:
        print(json.dumps({"id": issue, "title": title, "state": state, "claimed_by": claimed, "priority": "C", "body": "", "blocked_by": []}))
    else:
        print(f"{issue}  {state}  {title}")
        if claimed:
            print(f"claimed by {claimed}")
    sys.exit(0)
if cmd == "claim":
    issue = args[1]
    path, text = find(issue)
    if not path:
        sys.stderr.write("not found\n")
        sys.exit(1)
    pat = rf"\* (\w+)([^\n]*)\n:PROPERTIES:\n:ID:\s+({re.escape(issue)})\n(.*?):END:"
    def repl(m):
        rest, ident, props = m.group(2), m.group(3), m.group(4)
        props = re.sub(r":CLAIMED_BY:.*\n", "", props)
        return f"* STARTED{rest}\n:PROPERTIES:\n:ID:         {ident}\n:CLAIMED_BY: {who}\n{props}:END:"
    new, n = re.subn(pat, repl, text, count=1, flags=re.S)
    if n != 1:
        sys.stderr.write("claim parse failed\n")
        sys.exit(1)
    open(path, "w").write(new)
    print(f"claimed {issue} by {who}")
    sys.exit(0)
if cmd in ("note", "update", "recall"):
    sys.exit(0)
if cmd == "children":
    print("[]")
    sys.exit(0)
sys.exit(0)
"#,
        );
        Self { dir }
    }

    fn org(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("tracker/Software/ljos/issues.org")).unwrap()
    }

    /// `ljos` as conversation `conv`, from a shell whose parent shell
    /// already carries `OUTER` when `conv` is the inner id.
    fn ljos(&self, conv: &str, args: &[&str]) -> Output {
        self.ljos_as(conv, SEAT, args)
    }

    fn ljos_as(&self, conv: &str, seat: &str, args: &[&str]) -> Output {
        let ljos = env!("CARGO_BIN_EXE_ljos");
        let quoted: Vec<String> = std::iter::once(ljos.to_string())
            .chain(args.iter().map(|arg| shell_quote(arg)))
            .collect();
        let inner = format!("setsid -w {} < /dev/null", quoted.join(" "));
        let script = if conv == INNER {
            format!(
                "CURSOR_CONVERSATION_ID={INNER} bash --noprofile --norc -c {}",
                shell_quote(&inner)
            )
        } else {
            inner
        };
        let root = self.dir.path();
        let path = format!("{}:/usr/bin:/bin", root.join("bin").display());
        #[expect(
            clippy::disallowed_methods,
            reason = "starts a program outside git hook code"
        )]
        return Command::new("bash")
            .args(["--noprofile", "--norc", "-c", &script])
            .env_clear()
            .env("PATH", path)
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("home/config"))
            .env("XDG_STATE_HOME", root.join("home/state"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("TMPDIR", root.join("tmp"))
            .env("VISSUE_ROOT", root.join("tracker"))
            .env("VISSUE_NO_ROUTE", "1")
            .env("LJOS_SEAT", seat)
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("PACKSET_URL", "off")
            .env("PACKSET_AUTH", "off")
            .env("CURSOR_AGENT", "1")
            .env("CURSOR_CONVERSATION_ID", OUTER)
            .env("LJOS", ljos)
            .current_dir(root.join("home"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
    }
}

#[test]
fn claim_refusals_exit_one_and_name_the_next_command() {
    let world = Seat::new();
    let first = world.ljos(OUTER, &["claim", RPUD]);
    assert!(first.status.success(), "{}", text_of(&first));

    let refused = world.ljos_as(INNER, "other", &["claim", RPUD]);
    let stdout = String::from_utf8_lossy(&refused.stdout);
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{}", text_of(&refused));
    assert!(stdout.is_empty(), "{}", text_of(&refused));
    assert_eq!(
        stderr.lines().count(),
        1,
        "claim refusal was not one stderr line:\n{}",
        text_of(&refused)
    );
    assert!(
        stderr.contains(&format!("`ljos send {SEAT} TEXT`")),
        "{}",
        text_of(&refused)
    );
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn holder(conv: &str) -> String {
    ljos_cli::holder_for_session(SEAT, conv)
}

fn claimed(org: &str, issue: &str) -> (String, String) {
    let marker = format!(":ID:         {issue}");
    let rest = org
        .split_once(&marker)
        .unwrap_or_else(|| panic!("missing {issue} in {org}"))
        .1;
    let heading = org[..org.find(&marker).unwrap()]
        .lines()
        .rev()
        .find(|line| line.starts_with("* "))
        .unwrap_or("");
    let state = heading.split_whitespace().nth(1).unwrap_or("").to_string();
    let who = rest
        .lines()
        .take(6)
        .find_map(|line| line.trim().strip_prefix(":CLAIMED_BY:"))
        .map(|who| who.trim().to_string())
        .unwrap_or_default();
    (state, who)
}

#[test]
fn two_conversations_on_one_seat_claim_and_finish_their_own_tickets() {
    let seat = Seat::new();
    let outer = holder(OUTER);
    let inner = holder(INNER);
    assert_ne!(outer, inner);

    let sat = seat.ljos(OUTER, &["sitting", RPUD]);
    let said = text_of(&sat);
    assert!(sat.status.success(), "{said}");
    assert!(said.contains(&format!("{RPUD} is yours")), "{said}");
    let (state, who) = claimed(&seat.org(), RPUD);
    assert_eq!(state, "STARTED", "{said}\n{}", seat.org());
    assert_eq!(who, outer, "rpud was not claimed by the outer conversation");

    // The inner conversation has not sat. Finish with no id must not
    // take the outer sitting.
    let bare = seat.ljos(INNER, &["finish", "--lesson", "Nothing is open here."]);
    let bare_said = text_of(&bare);
    assert_eq!(bare.status.code(), Some(1), "{bare_said}");
    assert!(bare_said.contains(ljos_cli::NO_OPEN_SITTING), "{bare_said}");
    assert!(!bare_said.contains("finished"), "{bare_said}");
    assert!(!bare_said.contains("note written"), "{bare_said}");
    let (state, who) = claimed(&seat.org(), RPUD);
    assert_eq!((state.as_str(), who.as_str()), ("STARTED", outer.as_str()));

    let seat_line = seat.ljos(INNER, &["seat"]);
    let seat_said = text_of(&seat_line);
    assert!(seat_line.status.success(), "{seat_said}");
    assert!(
        seat_said.contains(&format!("holder\t{inner}")),
        "the inner conversation took the outer holder:\n{seat_said}"
    );

    let yw = seat.ljos(INNER, &["sitting", YW6T]);
    let yw_said = text_of(&yw);
    assert!(yw.status.success(), "{yw_said}");
    assert!(yw_said.contains(&format!("{YW6T} is yours")), "{yw_said}");
    assert!(!yw_said.contains("was not claimed"), "{yw_said}");
    let (state, who) = claimed(&seat.org(), YW6T);
    assert_eq!(state, "STARTED", "{yw_said}\n{}", seat.org());
    assert_eq!(
        who,
        inner,
        "yw6t was not claimed by this conversation:\n{}",
        seat.org()
    );
    let (state, who) = claimed(&seat.org(), RPUD);
    assert_eq!(
        (state.as_str(), who.as_str()),
        ("STARTED", outer.as_str()),
        "the inner sitting took rpud"
    );

    let shown = seat.ljos(INNER, &["show", YW6T]);
    let shown_said = text_of(&shown);
    assert!(shown.status.success(), "{shown_said}");
    assert!(shown_said.contains("STARTED"), "{shown_said}");
    assert!(shown_said.contains(&inner), "{shown_said}");
    assert!(!shown_said.contains("TODO"), "{shown_said}");

    let done = seat.ljos(
        INNER,
        &[
            "finish",
            YW6T,
            "--lesson",
            "The second conversation finishes its own ticket.",
        ],
    );
    let done_said = text_of(&done);
    assert!(done.status.success(), "{done_said}");
    assert!(
        done_said.contains(&format!("sitting ended; ticket {YW6T} is still open")),
        "{done_said}"
    );
    assert!(done_said.contains("sitting ended"), "{done_said}");
    assert!(
        !done_said.contains("note written"),
        "finish downgraded to a note:\n{done_said}"
    );
    assert!(
        !done_said.contains(&format!("your open sitting is {RPUD}")),
        "the outer sitting hijacked the inner finish:\n{done_said}"
    );
    let (state, who) = claimed(&seat.org(), RPUD);
    assert_eq!(
        (state.as_str(), who.as_str()),
        ("STARTED", outer.as_str()),
        "finish of yw6t released rpud:\n{}",
        seat.org()
    );
    let (state, _) = claimed(&seat.org(), YW6T);
    assert_ne!(state, "STARTED", "yw6t stayed claimed:\n{}", seat.org());
}
