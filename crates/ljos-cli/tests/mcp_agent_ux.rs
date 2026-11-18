//! What an agent that has never used ljos reads back from `ljos-mcp` when
//! it makes a typical mistake. Each test asserts the text a client shows.
//!
//! No pack, tracker or sibling tool is needed: every call here is answered
//! by the server itself.

ljos_test_boot::harness!();

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    _home: tempfile::TempDir,
}

impl Session {
    fn open(extra: &[(&str, &str)]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"));
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env("XDG_STATE_HOME", home.path().join("state"))
            .env("LJOS_MCP_ORPHAN_IDLE", "off")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_SEAT", "ada")
            .current_dir(home.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in extra {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let mut line = String::new();
            while stdout.read_line(&mut line).unwrap_or(0) > 0 {
                if tx.send(line.clone()).is_err() {
                    break;
                }
                line.clear();
            }
        });
        let mut session = Self {
            child,
            stdin,
            lines,
            next_id: 1,
            _home: home,
        };
        let init = session.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "probe", "version": "0"}
            }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "ljos", "{init}");
        session.send(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        session
    }

    fn send(&mut self, value: &serde_json::Value) {
        writeln!(self.stdin, "{value}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(12))
                .unwrap_or_else(|err| panic!("no answer to {method} ({err})"));
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id {
                return value;
            }
        }
    }

    fn call(&mut self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        self.request(
            "tools/call",
            serde_json::json!({"name": name, "arguments": arguments}),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The text blocks of a tool result, one per line.
fn blocks(value: &serde_json::Value) -> Vec<String> {
    value["result"]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("no content in {value}"))
        .iter()
        .filter_map(|block| block["text"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn a_bad_argument_names_the_field_that_failed() {
    let mut session = Session::open(&[]);
    let missing = session.call("ljos_claim", serde_json::json!({"seat": "ada"}));
    assert_eq!(missing["result"]["isError"], true, "{missing}");
    assert_eq!(
        blocks(&missing)[0],
        "ljos_claim: the arguments do not match the tool (missing field node), so run `ljos help claim`."
    );
    let wrong = session.call("ljos_claim", serde_json::json!({"seat": "ada", "node": 7}));
    assert_eq!(
        blocks(&wrong)[0],
        "ljos_claim: the arguments do not match the tool (invalid type: integer 7, expected a string), so run `ljos help claim`."
    );
    let command = session.call("ljos_guard_check", serde_json::json!({}));
    assert_eq!(
        blocks(&command)[0],
        "ljos_guard_check: the arguments do not match the tool (missing field command), so run `ljos help policy`."
    );
}

#[test]
fn a_failed_call_ends_with_a_next_line_that_fits_the_failure() {
    let mut session = Session::open(&[]);
    let guard = session.call("ljos_guard_check", serde_json::json!({"command": "  "}));
    assert_eq!(guard["result"]["isError"], true, "{guard}");
    let said = blocks(&guard);
    assert_eq!(said[0], "guard: pass the command to check", "{said:?}");
    assert_eq!(
        said[1],
        "next: do not run the command; fix the line above and call ljos_guard_check again"
    );
    let claim = session.call("ljos_claim", serde_json::json!({"seat": "ada"}));
    let said = blocks(&claim);
    assert_eq!(
        said.last().unwrap(),
        "next: fix the line above and call ljos_claim again"
    );
    let recall = session.call("ljos_recall", serde_json::json!({"issue": 7}));
    assert_eq!(
        blocks(&recall).last().unwrap(),
        "next: fix the line above and call ljos_recall again"
    );
    let allowed = session.call("ljos_guard_check", serde_json::json!({"command": "ls"}));
    assert_ne!(allowed["result"]["isError"], true, "{allowed}");
    assert_eq!(
        blocks(&allowed).last().unwrap(),
        "next: run the command, minding any warning line"
    );
}

#[test]
fn a_tool_that_is_not_served_says_what_to_do() {
    let mut session = Session::open(&[]);
    let unknown = session.call("ljos_remmeber", serde_json::json!({}));
    assert_eq!(unknown["result"]["isError"], true, "{unknown}");
    assert_eq!(
        blocks(&unknown)[0],
        "ljos_remmeber is not a tool, so call tools/list for the names this server serves, or run `ljos help`."
    );
    let unlisted = session.call("ljos_brief", serde_json::json!({}));
    assert_eq!(unlisted["result"]["isError"], true, "{unlisted}");
    assert_eq!(
        blocks(&unlisted),
        ["ljos_brief is not in this server's tool set. Ask the person to set LJOS_MCP_TOOLS=all (or core,brief) and restart the server, or run `ljos brief` in a shell."]
    );
    let plan = session.call("ljos_bump_plan", serde_json::json!({}));
    assert_eq!(
        blocks(&plan),
        ["ljos_bump_plan is not in this server's tool set. Ask the person to set LJOS_MCP_TOOLS=all (or core,bump_plan) and restart the server, or run `ljos bump-plan` in a shell."]
    );
    let mail = session.call("ljos_mail_show", serde_json::json!({}));
    assert_eq!(
        blocks(&mail),
        ["ljos_mail_show is not in this server's tool set. Ask the person to set LJOS_MCP_TOOLS=all (or core,mail_show) and restart the server."]
    );
}

#[test]
fn a_resource_is_read_as_the_type_the_listing_names() {
    let mut session = Session::open(&[]);
    let listed = session.request("resources/list", serde_json::json!({}));
    let resources = listed["result"]["resources"].as_array().expect("resources");
    assert_eq!(resources.len(), 3, "{listed}");
    for resource in resources {
        let uri = resource["uri"].as_str().unwrap();
        assert_eq!(resource["mimeType"], "text/markdown", "{uri}");
        let read = session.request("resources/read", serde_json::json!({"uri": uri}));
        assert_eq!(
            read["result"]["contents"][0]["mimeType"], "text/markdown",
            "{uri}: {read}"
        );
    }
}

/// Every `description` string under `value`, with the path it sits at.
fn descriptions(value: &serde_json::Value, at: &str, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                match child {
                    serde_json::Value::String(text) if key == "description" => {
                        out.push((at.to_string(), text.clone()));
                    }
                    _ => descriptions(child, &format!("{at}.{key}"), out),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                descriptions(item, &format!("{at}[{index}]"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn descriptions_are_one_line_of_single_spaces() {
    let mut session = Session::open(&[("LJOS_MCP_TOOLS", "all")]);
    let mut found = Vec::new();
    for method in ["tools/list", "prompts/list", "resources/list"] {
        let listed = session.request(method, serde_json::json!({}));
        assert!(listed["error"].is_null(), "{method}: {listed}");
        descriptions(&listed["result"], method, &mut found);
    }
    assert!(found.len() > 200, "only {} descriptions", found.len());
    for (at, text) in &found {
        assert!(
            !text.contains(char::is_control) && !text.contains("  ") && text == text.trim(),
            "{at} is not one line of single spaces: {text:?}"
        );
    }
}

/// The tools `tools/list` gives, by name.
fn listed_tools(session: &mut Session) -> std::collections::BTreeMap<String, serde_json::Value> {
    let listed = session.request("tools/list", serde_json::json!({}));
    listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| (tool["name"].as_str().unwrap().to_string(), tool.clone()))
        .collect()
}

fn description_of(
    tools: &std::collections::BTreeMap<String, serde_json::Value>,
    name: &str,
) -> String {
    tools[name]["description"].as_str().unwrap().to_string()
}

#[test]
fn the_longest_descriptions_say_what_to_pass_and_what_to_do_on_failure() {
    let mut session = Session::open(&[("LJOS_MCP_TOOLS", "all")]);
    let tools = listed_tools(&mut session);
    let longest = tools
        .keys()
        .map(|name| description_of(&tools, name).len())
        .max()
        .unwrap();
    assert!(longest < 1_300, "a description is {longest} characters");
    let search = description_of(&tools, "ljos_search");
    assert!(
        search.contains("A failure means the pack writer is down, so call ljos_doctor."),
        "{search}"
    );
    for word in ["as_of", "min_score", "project"] {
        assert!(
            search.contains(word),
            "search does not name {word}: {search}"
        );
    }
    assert!(!search.contains("cosine"), "{search}");
    let consensus = description_of(&tools, "ljos_consensus");
    assert!(
        consensus.starts_with("Call this after the ballots are in."),
        "{consensus}"
    );
    assert!(consensus.contains("Pass compare"), "{consensus}");
}

#[test]
fn claim_file_and_deed_each_describe_their_own_call() {
    let mut session = Session::open(&[("LJOS_MCP_TOOLS", "all")]);
    let tools = listed_tools(&mut session);
    for name in ["ljos_file", "ljos_claim", "ljos_deed"] {
        let said = description_of(&tools, name);
        assert!(said.starts_with("Call this "), "{name}: {said}");
        assert!(!said.contains("The seat already claims"), "{name}: {said}");
        assert!(!said.contains("occupies per issue"), "{name}: {said}");
    }
    let claim = description_of(&tools, "ljos_claim");
    assert!(
        claim.contains("ljos_start and ljos_sitting already claim"),
        "{claim}"
    );
    assert!(
        claim.contains(
            "a busy refusal names the held node, and ljos_complete or ljos_release frees it"
        ),
        "{claim}"
    );
    let deed = description_of(&tools, "ljos_deed");
    assert!(
        deed.contains("A git commit is cited as a deed for you"),
        "{deed}"
    );

    let prompt = session.request(
        "prompts/get",
        serde_json::json!({"name": "start_a_sitting", "arguments": {}}),
    );
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(!text.contains("occupies per issue"), "{text}");
    assert!(
        text.contains(
            "A named worker occupies one\nslot, and busy means it still has another node."
        ),
        "{text}"
    );
}

#[test]
fn sitting_says_how_it_differs_from_start_and_names_every_playbook() {
    let mut session = Session::open(&[("LJOS_MCP_TOOLS", "all")]);
    let tools = listed_tools(&mut session);
    let sitting = description_of(&tools, "ljos_sitting");
    assert!(
        sitting.contains("ljos_start is the shorter opening."),
        "{sitting}"
    );
    assert!(
        sitting.contains("Leave assignee out unless you run as a named worker."),
        "{sitting}"
    );
    assert!(!sitting.contains("Omit assignee"), "{sitting}");
    let names = "sit, arena, land, company-panel, overnight or feature";
    assert!(sitting.contains(names), "{sitting}");
    let param = tools["ljos_sitting"]["inputSchema"]["properties"]["playbook"]["description"]
        .as_str()
        .unwrap();
    assert!(
        param.ends_with("The recipe stays bound until `ljos_finish` or `ljos_release`."),
        "{param}"
    );
    for name in [
        "sit",
        "arena",
        "land",
        "company-panel",
        "overnight",
        "feature",
    ] {
        assert!(param.contains(&format!("`{name}`")), "{param}");
    }

    let prompt = session.request(
        "prompts/get",
        serde_json::json!({"name": "start_a_sitting", "arguments": {}}),
    );
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(
        text.contains("(sit, arena, land,\ncompany-panel, overnight or feature)"),
        "{text}"
    );
}
