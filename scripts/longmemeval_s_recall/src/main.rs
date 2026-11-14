//! Session recall on the public LongMemEval_S file, on a CPU, with no model.
//!
//! The file is downloaded into the scratch directory at run time and checked
//! against the pinned sha256. It is not stored in this repository.
//!
//! Abstention questions, whose ids end in `_abs`, are left out. Their labelled
//! sessions support a refusal, and the session-retrieval reading scores the
//! questions that have an answer in the history.
//!
//! Two arms:
//!
//! * `bm25+`, the pack's default lexical scorer (`packset_core::bm25::Scorer::Bm25Plus`).
//! * `island`, spreading activation over the name-sharing links those sessions
//!   would carry, seeded by the top BM25+ hits.
//!
//! Dense and fused arms are not scored. They need the encoder named in
//! `docs/scripts/packset-longmemeval-s.txt`, and this program does not load one.
//!
//! `--out DIR` writes `summary.json` and `per_question.csv` for the BM25+ arm.
//! The download names a dataset revision. `main` is not a pin.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use packset_core::search::Record;
use serde_json::{Map, Value};
use sha2::Digest;

/// Hugging Face dataset that publishes the cleaned LongMemEval_S file.
const DATA_REPO: &str = "xiaowu0162/longmemeval-cleaned";

/// Dataset commit that published `longmemeval_s_cleaned.json`. `main` moves.
const DATA_REV: &str = "98d7416c24c778c2fee6e6f3006e7a073259d48f";

/// File inside that revision. LongMemEval_S, cleaned.
const DATA_FILE: &str = "longmemeval_s_cleaned.json";

/// sha256 of that file, computed from the bytes the revision returned.
const DATA_SHA256: &str = "d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442";

/// How long a download may sit before it is a hung socket.
///
/// The file is a few hundred megabytes. Ten minutes is past a slow link.
const DOWNLOAD_TIMEOUT_S: u64 = 600;

/// Bytes copied from the socket at a time. This bounds the copy, not a score.
const DOWNLOAD_CHUNK: usize = 64 * 1024;

/// Depths at which hit and recall are reported.
///
/// One is the first session. Five is how many sessions the pack's
/// LongMemEval reading hands a reader. Ten is the further depth that
/// reading reports, so a miss at five is still visible.
const CUTOFFS: [usize; 3] = [1, 5, 10];

/// Search hits that seed spreading activation.
///
/// The pack's island is seeded by the top five search hits. Five is that
/// seed count, not a cutoff fitted to this file.
const ISLAND_SEEDS: usize = 5;

/// Hops of spreading activation.
///
/// The seat's island walks two hops along the links and returns the cluster
/// strongest first.
const ISLAND_HOPS: usize = 2;

/// packset-core this program links, the same minor the seat's crate pins.
const PACKSET_CORE: &str = "0.18.1";

struct Question {
    id: String,
    kind: String,
    text: String,
    sessions: Vec<(String, String)>,
    answers: BTreeSet<String>,
}

struct Row {
    id: String,
    kind: String,
    evidence: usize,
    /// Evidence sessions inside each cutoff, in `CUTOFFS` order.
    found: [usize; CUTOFFS.len()],
}

struct Tally {
    asked: usize,
    hit: [usize; CUTOFFS.len()],
    recall: [f64; CUTOFFS.len()],
}

impl Tally {
    fn new() -> Self {
        Self {
            asked: 0,
            hit: [0; CUTOFFS.len()],
            recall: [0.0; CUTOFFS.len()],
        }
    }

    fn add(&mut self, ranked: &[String], answers: &BTreeSet<String>) {
        self.asked += 1;
        let denom = answers.len().max(1) as f64;
        for (slot, found) in found_at(ranked, answers).into_iter().enumerate() {
            if found > 0 {
                self.hit[slot] += 1;
            }
            self.recall[slot] += found as f64 / denom;
        }
    }
}

/// Evidence sessions inside each cutoff.
fn found_at(ranked: &[String], answers: &BTreeSet<String>) -> [usize; CUTOFFS.len()] {
    let mut found = [0; CUTOFFS.len()];
    for (slot, cut) in CUTOFFS.iter().enumerate() {
        let top: BTreeSet<&str> = ranked.iter().take(*cut).map(String::as_str).collect();
        found[slot] = answers
            .iter()
            .filter(|id| top.contains(id.as_str()))
            .count();
    }
    found
}

fn data_url() -> String {
    format!("https://huggingface.co/datasets/{DATA_REPO}/resolve/{DATA_REV}/{DATA_FILE}")
}

fn main() -> Result<()> {
    let args = parse_args()?;
    let scored = score_question(&toy_question());
    anyhow::ensure!(
        scored.bm25.first().map(String::as_str) == Some("s-key"),
        "self-check: BM25+ did not rank the labelled session first"
    );
    anyhow::ensure!(
        scored.island.first().map(String::as_str) == Some("s-key"),
        "self-check: island order did not rank the labelled session first"
    );
    println!("self_check ok");
    if args.self_check {
        return Ok(());
    }

    let versions = lock_packset_versions(include_str!("../../../Cargo.lock"));
    anyhow::ensure!(
        versions == vec![PACKSET_CORE],
        "Cargo.lock packset-core versions {versions:?} are not {PACKSET_CORE}"
    );

    let path = dataset_path();
    download(&path)?;
    let started = Instant::now();
    let raw: Value = serde_json::from_reader(std::fs::File::open(&path).context("open dataset")?)
        .context("parse dataset")?;
    let items = raw
        .as_array()
        .context("dataset must be a JSON array of questions")?;

    let mut overall_bm25 = Tally::new();
    let mut overall_island = Tally::new();
    let mut by_kind: BTreeMap<String, (Tally, Tally)> = BTreeMap::new();
    let mut rows = Vec::new();
    let mut abstention = 0usize;
    let mut skipped = 0usize;
    for item in items {
        match parse_question(item) {
            Parsed::Abstention => abstention += 1,
            Parsed::Skip => skipped += 1,
            Parsed::Question(question) => {
                let scored = score_question(&question);
                overall_bm25.add(&scored.bm25, &question.answers);
                overall_island.add(&scored.island, &question.answers);
                rows.push(question_row(&question, &scored.bm25));
                let entry = by_kind
                    .entry(question.kind.clone())
                    .or_insert_with(|| (Tally::new(), Tally::new()));
                entry.0.add(&scored.bm25, &question.answers);
                entry.1.add(&scored.island, &question.answers);
                let n = overall_bm25.asked;
                if n % 50 == 0 {
                    eprintln!("{n} questions");
                }
            }
        }
    }

    println!("dataset_url {}", data_url());
    println!("dataset_repo {DATA_REPO}");
    println!("dataset_revision {DATA_REV}");
    println!("dataset_sha256 {DATA_SHA256}");
    println!("file_questions {}", items.len());
    println!("abstention_left_out {abstention}");
    println!("skipped {skipped}");
    println!("scored {}", overall_bm25.asked);
    println!("packset_core {PACKSET_CORE}");
    println!("ljos_version {}", ljos_version());
    println!("ljos_commit {}", git_head());
    println!("encoder none");
    println!("dense_arm not_scored");
    println!("fused_arm not_scored");
    println!(
        "metric hit@k share of scored questions with at least one evidence session in the top k"
    );
    println!(
        "metric recall@k mean over scored questions of the share of evidence sessions found in the top k"
    );
    let depths = CUTOFFS
        .iter()
        .map(|cut| cut.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    println!("k {depths}");
    println!("arms bm25+ island");
    print_machine();
    println!("{}", arm_line("arm ", "bm25+", &overall_bm25));
    println!("{}", arm_line("arm ", "island", &overall_island));
    for (kind, (bm25, island)) in &by_kind {
        let prefix = format!("type {kind} n {} arm ", bm25.asked);
        println!("{}", arm_line(&prefix, "bm25+", bm25));
        println!("{}", arm_line(&prefix, "island", island));
    }
    let wall = started.elapsed().as_secs_f64();
    println!("wall_s {wall:.3}");
    if let Some(dir) = args.out {
        write_run(&dir, &overall_bm25, &rows, wall)?;
    }
    Ok(())
}

struct Args {
    self_check: bool,
    out: Option<PathBuf>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        self_check: false,
        out: None,
    };
    let mut rest = std::env::args().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--self-check" => args.self_check = true,
            "--out" => {
                let path = rest.next().context("--out needs a directory")?;
                args.out = Some(PathBuf::from(path));
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(args)
}

fn question_row(question: &Question, ranked: &[String]) -> Row {
    Row {
        id: question.id.clone(),
        kind: question.kind.clone(),
        evidence: question.answers.len(),
        found: found_at(ranked, &question.answers),
    }
}

fn row_recall(row: &Row, slot: usize) -> f64 {
    let denom = row.evidence.max(1) as f64;
    row.found[slot] as f64 / denom
}

fn write_run(dir: &Path, bm25: &Tally, rows: &[Row], wall: f64) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    write_summary(&dir.join("summary.json"), bm25, wall)?;
    write_csv(&dir.join("per_question.csv"), rows)?;
    Ok(())
}

fn write_summary(path: &Path, tally: &Tally, wall: f64) -> Result<()> {
    let (hit, recall) = metric_at(tally, 5);
    let mut body = String::from("{\n");
    let mut comma = false;
    // The ratio and the wall time are the scorer's own digits, written as
    // JSON numbers so a later format pass cannot change them.
    push_number(&mut body, &mut comma, "hit@5", &hit)?;
    push_number(&mut body, &mut comma, "recall@5", &recall)?;
    push_number(&mut body, &mut comma, "questions", &tally.asked.to_string())?;
    push_number(&mut body, &mut comma, "seconds", &format!("{wall:.3}"))?;
    push_string(&mut body, &mut comma, "ljos", ljos_version())?;
    push_string(&mut body, &mut comma, "ljos_commit", &git_head())?;
    push_string(&mut body, &mut comma, "packset", PACKSET_CORE)?;
    push_string(&mut body, &mut comma, "dataset_repo", DATA_REPO)?;
    push_string(&mut body, &mut comma, "dataset_revision", DATA_REV)?;
    push_string(&mut body, &mut comma, "cpu_model", &cpu_model())?;
    push_string(&mut body, &mut comma, "date", &utc_date())?;
    body.push_str("\n}\n");
    std::fs::write(path, body).context("write summary.json")?;
    Ok(())
}

fn push_number(body: &mut String, comma: &mut bool, key: &str, number: &str) -> Result<()> {
    if number.is_empty()
        || !number
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'.'))
    {
        bail!("summary field {key} is not a decimal");
    }
    push_key(body, comma, key)?;
    body.push_str(number);
    Ok(())
}

fn push_string(body: &mut String, comma: &mut bool, key: &str, value: &str) -> Result<()> {
    push_key(body, comma, key)?;
    body.push_str(&serde_json::to_string(value).context("json string")?);
    Ok(())
}

fn push_key(body: &mut String, comma: &mut bool, key: &str) -> Result<()> {
    if *comma {
        body.push_str(",\n");
    }
    *comma = true;
    body.push_str("  ");
    body.push_str(&serde_json::to_string(key).context("json key")?);
    body.push_str(": ");
    Ok(())
}

fn write_csv(path: &Path, rows: &[Row]) -> Result<()> {
    let mut file = std::fs::File::create(path).context("write per_question.csv")?;
    writeln!(
        file,
        "question_id,question_type,evidence,found@1,hit@1,recall@1,found@5,hit@5,recall@5,found@10,hit@10,recall@10"
    )?;
    for row in rows {
        writeln!(
            file,
            "{},{},{},{},{},{},{},{},{},{},{},{}",
            csv_escape(&row.id),
            csv_escape(&row.kind),
            row.evidence,
            row.found[0],
            u8::from(row.found[0] > 0),
            ratio(row_recall(row, 0), 1),
            row.found[1],
            u8::from(row.found[1] > 0),
            ratio(row_recall(row, 1), 1),
            row.found[2],
            u8::from(row.found[2] > 0),
            ratio(row_recall(row, 2), 1),
        )?;
    }
    Ok(())
}

fn metric_at(tally: &Tally, cut: usize) -> (String, String) {
    let slot = CUTOFFS
        .iter()
        .position(|depth| *depth == cut)
        .expect("cutoff is scored");
    (
        ratio(tally.hit[slot] as f64, tally.asked),
        ratio(tally.recall[slot], tally.asked),
    )
}

fn csv_escape(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn ljos_version() -> &'static str {
    include_str!("../../../Cargo.toml")
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .unwrap_or("unknown")
}

fn utc_date() -> String {
    let stamp = packset_core::clock::utcnow();
    stamp.split('T').next().unwrap_or("unknown").to_string()
}

struct Scored {
    bm25: Vec<String>,
    island: Vec<String>,
}

fn score_question(question: &Question) -> Scored {
    let mut atoms = Vec::with_capacity(question.sessions.len());
    let mut docs = Vec::with_capacity(question.sessions.len());
    let mut ids = Vec::with_capacity(question.sessions.len());
    for (sid, text) in &question.sessions {
        ids.push(sid.clone());
        docs.push(packset_core::search::tokens(text));
        atoms.push(session_atom(sid, text));
    }
    link_atoms(&mut atoms);
    let query = packset_core::search::tokens(&question.text);
    let lexical = bm25_plus(&docs, &query);
    let bm25 = lexical
        .iter()
        .map(|(index, _)| ids[*index].clone())
        .collect();
    let island = island_order(&atoms, &lexical)
        .into_iter()
        .map(|index| ids[index].clone())
        .collect();
    Scored { bm25, island }
}

fn session_atom(sid: &str, text: &str) -> Record {
    let mut atom = Map::new();
    atom.insert("id".into(), Value::String(sid.to_string()));
    atom.insert("text".into(), Value::String(text.to_string()));
    let names: Vec<Value> = packset_core::record::entities_of(&atom)
        .into_iter()
        .map(Value::String)
        .collect();
    atom.insert("entities".into(), Value::Array(names));
    // Links read the names. Dropping the text keeps the haystack from sitting
    // in the graph as well as in the token lists.
    atom.remove("text");
    atom
}

fn link_atoms(atoms: &mut [Record]) {
    let now = packset_core::clock::utcnow();
    let borrowed: Vec<&Record> = atoms.iter().collect();
    let lists: Vec<Vec<String>> = atoms
        .iter()
        .map(|atom| {
            packset_core::record::link_targets(
                atom,
                &borrowed,
                packset_core::record::LINK_THRESHOLD,
                &now,
            )
        })
        .collect();
    for (atom, links) in atoms.iter_mut().zip(lists) {
        atom.insert(
            "links".into(),
            Value::Array(links.into_iter().map(Value::String).collect()),
        );
    }
}

/// BM25+ over the session documents, best first.
///
/// `k1`, `b` and the floor live in packset-core. Each query token is passed
/// once per occurrence, with weight one, which is the unweighted query the
/// pack's search scores.
fn bm25_plus(docs: &[Vec<String>], query: &[String]) -> Vec<(usize, f64)> {
    let index = packset_core::bm25::Index::build(docs.iter().map(Vec::as_slice));
    let weighted: Vec<(String, f64)> = query.iter().cloned().map(|term| (term, 1.0)).collect();
    let mut scored = index.score_weighted_by(packset_core::bm25::Scorer::Bm25Plus, &weighted);
    scored.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));
    scored
}

fn island_order(atoms: &[Record], lexical: &[(usize, f64)]) -> Vec<usize> {
    let graph = packset_core::island::Graph::from_atoms(atoms);
    let seeds: Vec<(usize, f64)> = lexical.iter().copied().take(ISLAND_SEEDS).collect();
    packset_core::island::activate(&graph, &seeds, ISLAND_HOPS)
        .into_iter()
        .map(|(index, _score)| index)
        .collect()
}

enum Parsed {
    Abstention,
    Skip,
    Question(Question),
}

fn parse_question(value: &Value) -> Parsed {
    let Some(id) = value.get("question_id").and_then(Value::as_str) else {
        return Parsed::Skip;
    };
    // The benchmark marks an abstention question by this suffix on the id.
    if id.ends_with("_abs") {
        return Parsed::Abstention;
    }
    let Some(text) = value.get("question").and_then(Value::as_str) else {
        return Parsed::Skip;
    };
    let Some(ids) = value.get("haystack_session_ids").and_then(Value::as_array) else {
        return Parsed::Skip;
    };
    let Some(sessions) = value.get("haystack_sessions").and_then(Value::as_array) else {
        return Parsed::Skip;
    };
    let answers: BTreeSet<String> = value
        .get("answer_session_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if answers.is_empty() {
        return Parsed::Skip;
    }
    let sessions = ids
        .iter()
        .zip(sessions)
        .filter_map(|(sid, turns)| Some((sid.as_str()?.to_string(), session_text(turns))))
        .collect();
    Parsed::Question(Question {
        id: id.to_string(),
        kind: value
            .get("question_type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        text: text.to_string(),
        sessions,
        answers,
    })
}

/// One session as a document: each turn on its own line, role then content.
fn session_text(turns: &Value) -> String {
    let Some(turns) = turns.as_array() else {
        return String::new();
    };
    let mut lines = Vec::with_capacity(turns.len());
    for turn in turns {
        let role = turn.get("role").and_then(Value::as_str).unwrap_or("");
        let Some(content) = turn.get("content").and_then(Value::as_str) else {
            continue;
        };
        lines.push(format!("{role}: {content}"));
    }
    lines.join("\n")
}

fn dataset_path() -> PathBuf {
    let mut dir = std::env::var_os("LJOS_LME_SCRATCH")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.push("ljos-longmemeval-s");
    dir.push("longmemeval_s_cleaned.json");
    dir
}

fn download(dest: &Path) -> Result<()> {
    if dest.is_file() && file_sha256(dest)? == DATA_SHA256 {
        eprintln!("dataset sha256 matches the pin");
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    eprintln!("downloading dataset");
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_S))
        .build();
    let response = agent.get(&data_url()).call().context("GET dataset")?;
    let mut reader = response.into_reader();
    let partial = dest.with_extension("partial");
    let mut file = std::fs::File::create(&partial).context("create dataset file")?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; DOWNLOAD_CHUNK];
    loop {
        let read = reader.read(&mut buf).context("read dataset")?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        file.write_all(&buf[..read])?;
    }
    file.sync_all()?;
    drop(file);
    let got = hex_bytes(&hasher.finalize());
    if got != DATA_SHA256 {
        let _ = std::fs::remove_file(&partial);
        bail!("sha256 {got} does not match the pin {DATA_SHA256}");
    }
    std::fs::rename(&partial, dest).context("rename dataset file")?;
    eprintln!("dataset sha256 matches the pin");
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; DOWNLOAD_CHUNK];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex_bytes(&hasher.finalize()))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn lock_packset_versions(lock: &str) -> Vec<&str> {
    let mut versions = Vec::new();
    let mut saw_name = false;
    for line in lock.lines() {
        if line == "name = \"packset-core\"" {
            saw_name = true;
            continue;
        }
        if saw_name {
            if let Some(version) = line
                .strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
            {
                versions.push(version);
            }
            saw_name = false;
        }
    }
    versions
}

fn git_head() -> String {
    let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
    else {
        return "unknown".to_string();
    };
    if !output.status.success() {
        return "unknown".to_string();
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let rest = line.strip_prefix("model name")?;
                rest.split(':').nth(1).map(|value| value.trim().to_string())
            })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn print_machine() {
    let model = cpu_model();
    let cores = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(0);
    let mem = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|line| line.starts_with("MemTotal:"))
                .map(str::trim)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "MemTotal: unknown".to_string());
    println!("cpu_model {model}");
    println!("cpu_cores {cores}");
    println!("mem_total {mem}");
}

fn arm_line(prefix: &str, arm: &str, tally: &Tally) -> String {
    let mut line = format!("{prefix}{arm}");
    for (slot, cut) in CUTOFFS.iter().enumerate() {
        let hit = ratio(tally.hit[slot] as f64, tally.asked);
        let recall = ratio(tally.recall[slot], tally.asked);
        line.push_str(&format!(" hit@{cut} {hit} recall@{cut} {recall}"));
    }
    line
}

/// Six digits past the point. One part in a million is finer than one
/// question in this file, so two different counts do not print the same.
fn ratio(part: f64, whole: usize) -> String {
    if whole == 0 {
        return "0.000000".to_string();
    }
    format!("{:.6}", part / whole as f64)
}

fn toy_question() -> Question {
    Question {
        id: "q-toy".to_string(),
        kind: "toy".to_string(),
        text: "Where does Alice keep the spare key?".to_string(),
        sessions: vec![
            (
                "s-key".to_string(),
                "user: Alice keeps the spare key under the blue mat.".to_string(),
            ),
            (
                "s-weather".to_string(),
                "user: Bob talked about the weather and the market.".to_string(),
            ),
            (
                "s-bread".to_string(),
                "user: Carol baked bread on Sunday.".to_string(),
            ),
        ],
        answers: BTreeSet::from(["s-key".to_string()]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toy_question_recalls_the_labelled_session() {
        let scored = score_question(&toy_question());
        assert_eq!(scored.bm25.first().map(String::as_str), Some("s-key"));
        assert_eq!(scored.island.first().map(String::as_str), Some("s-key"));
    }

    #[test]
    fn toy_row_hits_at_five() {
        let question = toy_question();
        let scored = score_question(&question);
        let row = question_row(&question, &scored.bm25);
        assert_eq!(row.id, "q-toy");
        assert_eq!(row.evidence, 1);
        assert_eq!(row.found[1], 1);
        assert_eq!(row_recall(&row, 1), 1.0);
    }

    #[test]
    fn lock_names_the_pinned_packset_core() {
        let versions = lock_packset_versions(include_str!("../../../Cargo.lock"));
        assert_eq!(versions, vec![PACKSET_CORE]);
    }

    #[test]
    fn dataset_url_pins_the_revision() {
        let url = data_url();
        assert!(url.contains(DATA_REV));
        assert!(url.contains(DATA_REPO));
        assert!(url.ends_with(DATA_FILE));
        assert!(!url.contains("/main/"));
    }

    #[test]
    fn summary_names_the_bm25_run() {
        let mut tally = Tally::new();
        tally.asked = 2;
        tally.hit[1] = 1;
        tally.recall[1] = 1.5;
        let dir = std::env::temp_dir().join(format!("ljos-lme-summary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_summary(&dir.join("summary.json"), &tally, 1.25).unwrap();
        let text = std::fs::read_to_string(dir.join("summary.json")).unwrap();
        assert!(text.starts_with("{\n  \"hit@5\": 0.500000"));
        assert!(text.contains("\"recall@5\": 0.750000"));
        assert!(text.contains("\"questions\": 2"));
        assert!(text.contains("\"seconds\": 1.250"));
        assert!(text.contains("\"packset\": \"0.18.1\""));
        assert!(text.contains(DATA_REV));
        let _ = std::fs::remove_dir_all(dir);
    }
}
