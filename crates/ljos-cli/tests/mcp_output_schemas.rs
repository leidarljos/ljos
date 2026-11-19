//! A tool's `structuredContent` is what its `outputSchema` says it is.
//!
//! The write tools that pass the pack's atom through, `ljos_rule`,
//! `ljos_prefer`, `ljos_forget`, `ljos_habit`, `ljos_predict`, `ljos_trust`
//! and `ljos_persona`, list their fields instead of an open object. Each
//! call here runs against a real pack writer, and the result is checked
//! against the listed schema. A field the writer adds and the schema leaves
//! out fails the check, so the list cannot drift from the answer.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

const SEVEN: [&str; 7] = [
    "ljos_rule",
    "ljos_prefer",
    "ljos_forget",
    "ljos_habit",
    "ljos_predict",
    "ljos_trust",
    "ljos_persona",
];

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn type_fits(want: &str, value: &Value) -> bool {
    let have = kind_of(value);
    have == want || (want == "number" && have == "integer")
}

fn resolve<'a>(root: &'a Value, reference: &str) -> &'a Value {
    let path = reference
        .strip_prefix("#/")
        .unwrap_or_else(|| panic!("unsupported reference {reference}"));
    path.split('/').fold(root, |at, key| {
        at.get(key)
            .unwrap_or_else(|| panic!("{reference} is not in the schema"))
    })
}

/// The JSON Schema keywords the listed schemas use. A property the schema
/// does not list is an error, whether or not the schema says more may come.
fn check(root: &Value, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
    let map = match schema {
        Value::Bool(true) => return,
        Value::Bool(false) => {
            errors.push(format!("{at}: the schema allows nothing"));
            return;
        }
        Value::Object(map) => map,
        other => panic!("{at}: not a schema: {other}"),
    };
    if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
        return check(root, resolve(root, reference), value, at, errors);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(options) = map.get(key).and_then(Value::as_array) {
            let fits = options.iter().any(|option| {
                let mut tried = Vec::new();
                check(root, option, value, at, &mut tried);
                tried.is_empty()
            });
            if !fits {
                errors.push(format!("{at}: {value} fits none of {key}"));
            }
        }
    }
    if let Some(all) = map.get("allOf").and_then(Value::as_array) {
        for part in all {
            check(root, part, value, at, errors);
        }
    }
    if let Some(want) = map.get("type") {
        let wants: Vec<&str> = match want {
            Value::String(one) => vec![one.as_str()],
            Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
            other => panic!("{at}: bad type {other}"),
        };
        if !wants.iter().any(|want| type_fits(want, value)) {
            errors.push(format!("{at}: {value} is not {wants:?}"));
            return;
        }
    }
    if let Some(options) = map.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            errors.push(format!("{at}: {value} is not one of {options:?}"));
        }
    }
    if let (Some(min), Some(n)) = (
        map.get("minimum").and_then(Value::as_f64),
        value.as_f64().filter(|_| value.is_number()),
    ) {
        if n < min {
            errors.push(format!("{at}: {n} is under {min}"));
        }
    }
    if let (Some(items), Some(list)) = (map.get("items"), value.as_array()) {
        for (i, item) in list.iter().enumerate() {
            check(root, items, item, &format!("{at}[{i}]"), errors);
        }
    }
    if let Some(object) = value.as_object() {
        let props = map.get("properties").and_then(Value::as_object);
        if let Some(required) = map.get("required").and_then(Value::as_array) {
            for name in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(name) {
                    errors.push(format!("{at}: {name} is required and missing"));
                }
            }
        }
        for (name, member) in object {
            let here = format!("{at}.{name}");
            match props.and_then(|p| p.get(name)) {
                Some(schema) => check(root, schema, member, &here, errors),
                None => match map.get("additionalProperties") {
                    Some(Value::Object(_)) => {
                        check(root, &map["additionalProperties"], member, &here, errors);
                    }
                    Some(Value::Bool(true)) | None if props.is_none() => {}
                    _ => errors.push(format!("{here}: not a listed field")),
                },
            }
        }
    }
}

fn validate(schema: &Value, value: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    check(schema, schema, value, "$", &mut errors);
    errors
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}

impl Session {
    fn open(home: &std::path::Path, pack: &str) -> Self {
        let run = home.join("run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(home.join(".config")).unwrap();
        std::fs::create_dir_all(home.join("tracker")).unwrap();
        let gitconfig = home.join("gitconfig");
        std::fs::write(&gitconfig, "").unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ljos-mcp"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home)
            .env("USER", "person")
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_RUNTIME_DIR", &run)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("VISSUE_ROOT", home.join("tracker"))
            .env("LJOS_SEAT", "ada")
            .env("LJOS_MCP_TOOLS", "all")
            .env("LJOS_TRACKER_GIT", "off")
            .env("LJOS_HOOKD", "off")
            .env("LJOS_USER_RULES_SYNC", "off")
            .env("PACKSET_URL", pack)
            .env("PACKSET_WORKSPACE", "seat")
            .env("PACKSET_AUTH", "off")
            .env("PACKSET_TIMEOUT_MS", "5000")
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(120));
            let _ = Command::new("kill").arg(pid.to_string()).status();
        });
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut session = Self {
            child,
            stdin,
            stdout,
            next: 1,
        };
        session.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "probe", "version": "0"}}),
        );
        writeln!(
            session.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .unwrap();
        session
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).unwrap();
            assert!(n > 0, "ljos-mcp closed before answering {method}");
            if let Ok(answer) = serde_json::from_str::<Value>(&line) {
                if answer["id"] == id {
                    return answer["result"].clone();
                }
            }
        }
    }

    fn tools(&mut self) -> Vec<Value> {
        self.request("tools/list", json!({}))["tools"]
            .as_array()
            .unwrap()
            .clone()
    }

    fn call(&mut self, name: &str, mut args: Value) -> Value {
        args["seat"] = json!("ada");
        self.request("tools/call", json!({"name": name, "arguments": args}))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn http_get(url: &str, path: &str) -> String {
    let addr = url.trim_start_matches("http://");
    let mut stream = TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    ljos_test_boot::http_body(&raw).unwrap()
}

fn lesson_ids(url: &str, short: &str) -> String {
    let body = http_get(url, "/v1/atoms?workspace=seat&kind=lesson");
    let all: Value = serde_json::from_str(&body).unwrap();
    all["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|atom| atom["id"].as_str())
        .find(|id| id.starts_with(short))
        .unwrap_or_else(|| panic!("no lesson {short} in {body}"))
        .to_string()
}

fn short_id(text: &str) -> String {
    let at = text.find("\n  id ").expect("remember names an id") + "\n  id ".len();
    text[at..]
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect()
}

fn schema_of<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not listed"))
        .get("outputSchema")
        .unwrap_or_else(|| panic!("{name} has no outputSchema"))
}

fn answer<'a>(name: &str, result: &'a Value) -> &'a Value {
    assert!(
        result["isError"] != true,
        "{name} failed: {}",
        result["content"]
    );
    &result["structuredContent"]
}

fn open_object(schema: &Value) -> bool {
    match schema {
        Value::Object(map) => {
            map.get("additionalProperties") == Some(&Value::Bool(true))
                || map.values().any(open_object)
        }
        Value::Array(items) => items.iter().any(open_object),
        _ => false,
    }
}

#[test]
fn the_seven_write_tools_list_their_fields() {
    if tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let pack = ljos_test_boot::Packsetd::open();
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::open(home.path(), pack.url());
    let listed = session.tools();
    for name in SEVEN {
        let schema = schema_of(&listed, name);
        assert!(
            !open_object(schema),
            "{name} lists an open object: {schema}"
        );
        let props = schema["properties"].as_object().unwrap();
        assert!(props.len() >= 3, "{name} lists {} fields", props.len());
        assert!(
            !props.contains_key("value") || name == "ljos_habit",
            "{name} carries a wrapped value"
        );
    }

    let mut results: Vec<(&str, Value)> = Vec::new();
    let mut run = |session: &mut Session, name: &'static str, args: Value| {
        let result = session.call(name, args);
        results.push((name, answer(name, &result).clone()));
        result
    };

    run(
        &mut session,
        "ljos_rule",
        json!({"pattern": "git push --force*", "verdict": "ask", "why": "force pushes lose work"}),
    );
    run(
        &mut session,
        "ljos_rule",
        json!({"pattern": "rm -rf /tmp/x*", "verdict": "deny", "why": "scratch", "user": true}),
    );

    run(
        &mut session,
        "ljos_prefer",
        json!({"text": "Use short names"}),
    );
    let user = run(
        &mut session,
        "ljos_prefer",
        json!({"text": "Use tabs in Makefiles", "user": true, "why": "the person said so"}),
    );
    let rule_id = user["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(
        &mut session,
        "ljos_prefer",
        json!({"user": true, "forget": rule_id, "text": ""}),
    );
    let lesson = session.call("ljos_remember", json!({"text": "Prefer small commits."}));
    let said = lesson["structuredContent"]["text"].as_str().unwrap();
    let full = lesson_ids(pack.url(), &short_id(said));
    run(
        &mut session,
        "ljos_prefer",
        json!({"user": true, "promote": full, "why": "the person asked"}),
    );

    let doomed = session.call(
        "ljos_remember",
        json!({"text": "Run the tests before a push."}),
    );
    let doomed = short_id(doomed["structuredContent"]["text"].as_str().unwrap());
    run(&mut session, "ljos_forget", json!({"id": doomed}));

    run(
        &mut session,
        "ljos_habit",
        json!({"name": "latency", "value": 12.5, "unit": "ms", "every": "7d", "source": "bench"}),
    );
    run(
        &mut session,
        "ljos_habit",
        json!({"name": "latency", "value": 11.0, "unit": "ms"}),
    );
    run(&mut session, "ljos_habit", json!({}));
    run(&mut session, "ljos_habit", json!({"name": "no-such-habit"}));

    run(
        &mut session,
        "ljos_predict",
        json!({"issue": "proj-1a2b", "expect": "accept"}),
    );
    run(
        &mut session,
        "ljos_predict",
        json!({"issue": "proj-1a2b", "expect": {"accept": 0.6, "reject": 0.4}}),
    );
    run(
        &mut session,
        "ljos_persona",
        json!({"name": "critic", "view": "reads for the weakest claim", "anchor": 0.3,
            "about": ["docs"]}),
    );
    run(
        &mut session,
        "ljos_trust",
        json!({"from": "ada", "to": "critic", "weight": 0.7}),
    );
    run(
        &mut session,
        "ljos_trust",
        json!({"from": "ada", "to": "critic", "weight": 0.8, "about": ["docs"]}),
    );

    let mut seen: Vec<&str> = Vec::new();
    for (name, body) in &results {
        let errors = validate(schema_of(&listed, name), body);
        assert!(errors.is_empty(), "{name}: {errors:#?}\n{body}");
        if !seen.contains(name) {
            seen.push(name);
        }
    }
    for name in SEVEN {
        assert!(seen.contains(&name), "{name} was never called");
    }
}

#[test]
fn a_sync_report_and_an_imported_memory_fit_their_schemas() {
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::open(home.path(), "off");
    let listed = session.tools();
    let sync = json!({"report": "pushed 1 user rule to the tracker"});
    assert_eq!(
        validate(schema_of(&listed, "ljos_prefer"), &sync),
        Vec::<String>::new()
    );
    let retired = json!({"id": "local:imp:0:ab12cd34", "kind": "lesson",
        "text": "An imported lesson.", "tombstone": true, "due_at": ""});
    assert_eq!(
        validate(schema_of(&listed, "ljos_forget"), &retired),
        Vec::<String>::new()
    );
    let mistyped = json!({"id": "ur-1", "text": "x", "pattern": "p", "verdict": 3});
    assert!(!validate(schema_of(&listed, "ljos_rule"), &mistyped).is_empty());
    let extra = json!({"id": "a", "text": "x", "pattern": "p", "verdict": "ask", "colour": "red"});
    assert!(!validate(schema_of(&listed, "ljos_rule"), &extra).is_empty());
    let missing = json!({"id": "a", "text": "x"});
    assert!(!validate(schema_of(&listed, "ljos_rule"), &missing).is_empty());
}

#[test]
fn a_read_only_tool_result_fits_its_schema() {
    if tools::halt(tools::Tool::Packsetd) {
        return;
    }
    let pack = ljos_test_boot::Packsetd::open();
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::open(home.path(), pack.url());
    session.call(
        "ljos_persona",
        json!({"name": "critic", "view": "reads for the weakest claim", "anchor": 0.3}),
    );
    session.call(
        "ljos_habit",
        json!({"name": "latency", "value": 12.5, "unit": "ms"}),
    );
    let listed = session.tools();
    let mut checked = Vec::new();
    for tool in &listed {
        let name = tool["name"].as_str().unwrap();
        if tool["annotations"]["readOnlyHint"] != true || tool.get("outputSchema").is_none() {
            continue;
        }
        let result = session.call(name, json!({}));
        if result["isError"] == true {
            continue;
        }
        let body = &result["structuredContent"];
        if body.is_null() {
            continue;
        }
        let errors = validate(&tool["outputSchema"], body);
        assert!(errors.is_empty(), "{name}: {errors:#?}\n{body}");
        checked.push(name.to_string());
    }
    for name in ["ljos_doctor", "ljos_personas", "ljos_due"] {
        assert!(
            checked.iter().any(|c| c == name),
            "{name} was not checked: {checked:?}"
        );
    }
}
