//! stdio MCP that execs `ljos`. Agents load this instead of four servers.

use std::io::{self, BufRead, Write};
use std::process::Command;

fn main() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: serde_json::Value = serde_json::from_str(&line)?;
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let resp = match method {
            "initialize" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "ljos", "version": "0.1.0" }
                }
            }),
            "tools/list" => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [
                    {"name": "ljos_remember", "description": "Standing knowledge. Remember: only. POST /v1/atoms. Never extract-on-write.", "inputSchema": {"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}},
                    {"name": "ljos_prefer", "description": "Standing preference. Prefer: only. POST /v1/atoms. Never extract-on-write.", "inputSchema": {"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}},
                    {"name": "ljos_search", "description": "Retrieve standing knowledge via PACKSET_URL.", "inputSchema": {"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}},
                    {"name": "ljos_evidence", "description": "deedar evidence for an accession.", "inputSchema": {"type":"object","properties":{"accession":{"type":"string"}},"required":["accession"]}},
                    {"name": "ljos_current", "description": "deedar current tip for an accession.", "inputSchema": {"type":"object","properties":{"accession":{"type":"string"}},"required":["accession"]}},
                    {"name": "ljos_deed", "description": "Cite an accession on a tracker node.", "inputSchema": {"type":"object","properties":{"issue":{"type":"string"},"add":{"type":"string"}},"required":["issue"]}},
                    {"name": "ljos_recall", "description": "Working set for a tracker node.", "inputSchema": {"type":"object","properties":{"issue":{"type":"string"}},"required":["issue"]}},
                    {"name": "ljos_claim", "description": "Claim a claimdag node. Completing does not close a ticket.", "inputSchema": {"type":"object","properties":{"node":{"type":"string"}},"required":["node"]}},
                    {"name": "ljos_complete", "description": "Complete a claimdag node. Completing does not close a ticket.", "inputSchema": {"type":"object","properties":{"node":{"type":"string"}},"required":["node"]}},
                    {"name": "ljos_cards", "description": "Read-only cards. USER.md and MEMORY.md only. Never write.", "inputSchema": {"type":"object","properties":{}}},
                    {"name": "ljos_policy", "description": "Print argv. grok-policyd is the TCB when present. Reloading a pack is not a check.", "inputSchema": {"type":"object","properties":{"argv":{"type":"array","items":{"type":"string"}}},"required":["argv"]}},
                    {"name": "ljos_consensus", "description": "ljos-consensus then vissue consensus. Not a vote count.", "inputSchema": {"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}
                ]}
            }),
            "tools/call" => {
                let name = req["params"]["name"].as_str().unwrap_or("");
                let args = &req["params"]["arguments"];
                let out = call(name, args);
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{"type":"text","text": out}] }
                })
            }
            "notifications/initialized" => continue,
            _ => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": method }
            }),
        };
        writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.flush()?;
    }
    Ok(())
}

fn call(name: &str, args: &serde_json::Value) -> String {
    let mut cmd = Command::new("ljos");
    match name {
        "ljos_remember" => {
            cmd.arg("remember").arg(args["text"].as_str().unwrap_or(""));
        }
        "ljos_prefer" => {
            cmd.arg("prefer").arg(args["text"].as_str().unwrap_or(""));
        }
        "ljos_search" => {
            cmd.arg("search").arg(args["query"].as_str().unwrap_or(""));
        }
        "ljos_evidence" => {
            cmd.arg("evidence")
                .arg(args["accession"].as_str().unwrap_or(""));
        }
        "ljos_current" => {
            cmd.arg("current")
                .arg(args["accession"].as_str().unwrap_or(""));
        }
        "ljos_deed" => {
            cmd.arg("deed").arg(args["issue"].as_str().unwrap_or(""));
            if let Some(a) = args["add"].as_str() {
                cmd.arg("--add").arg(a);
            }
        }
        "ljos_recall" => {
            cmd.arg("recall").arg(args["issue"].as_str().unwrap_or(""));
        }
        "ljos_claim" => {
            cmd.arg("claim").arg(args["node"].as_str().unwrap_or(""));
        }
        "ljos_complete" => {
            cmd.arg("complete").arg(args["node"].as_str().unwrap_or(""));
        }
        "ljos_cards" => {
            cmd.arg("cards");
        }
        "ljos_policy" => {
            cmd.arg("policy");
            if let Some(v) = args["argv"].as_array() {
                for a in v {
                    if let Some(s) = a.as_str() {
                        cmd.arg(s);
                    }
                }
            }
        }
        "ljos_consensus" => {
            cmd.arg("consensus").arg(args["id"].as_str().unwrap_or(""));
        }
        other => return format!("unknown tool {other}"),
    }
    match cmd.output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        }
        Err(e) => e.to_string(),
    }
}
