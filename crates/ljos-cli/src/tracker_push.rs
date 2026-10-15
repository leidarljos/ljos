//! Push a tracker checkout without losing another bot's headings.
//!
//! Seven seats share one tracker. Two of them append a heading, and the
//! second push is rejected as non-fast-forward. The push fetches, rebases
//! with autostash, and retries. When the org file conflicts, both headings
//! stay: one entry per `:ID:`. Nothing is force-pushed.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Output;

const ATTEMPTS: usize = 4;

/// Commit `dir`'s branch and the org headings already on the remote.
///
/// A branch with no upstream is pushed with `-u origin <branch>`. A
/// non-fast-forward fetch is rebased with `--autostash` and tried again.
/// A conflict in an issue org file keeps every heading, keyed by `:ID:`.
///
/// # Errors
///
/// The push still failed after a few attempts, or a conflict was not an
/// org file this can union.
pub(crate) fn push_checkout(dir: &Path) -> Result<(), String> {
    let mut last = String::from("push refused");
    for _ in 0..ATTEMPTS {
        let out = push_once(dir);
        if out.status.success() {
            return push_mirrors(dir);
        }
        let said = output_text(&out);
        last = first_line(&said);
        if !rejected(&said) {
            return Err(last);
        }
        integrate(dir).map_err(|e| {
            last = e.clone();
            e
        })?;
    }
    Err(format!(
        "{last}; still not a fast-forward after {ATTEMPTS} attempts"
    ))
}

/// The shell that makes one push attempt. The parent retries a rejection
/// in-process, where an org conflict can be unioned.
pub(crate) fn push_script(dir: &Path, mirrors: &[(String, String)]) -> String {
    let push = primary_push(dir);
    let mut script = format!("{push}; rc=$?");
    if !mirrors.is_empty() {
        script.push_str("; if [ \"$rc\" -eq 0 ]; then");
        for (remote, branch) in mirrors {
            let spec = format!("HEAD:refs/heads/{branch}");
            script.push_str(&format!(
                " git push -q {} {} || rc=1;",
                crate::tools::sh_quote(remote),
                crate::tools::sh_quote(&spec)
            ));
        }
        script.push_str(" fi");
    }
    script.push_str("; exit $rc");
    script
}

/// Whether git refused the update because the remote moved.
#[must_use]
pub(crate) fn rejected(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("non-fast-forward")
        || lower.contains("failed to push some refs")
        || lower.contains("updates were rejected")
        || lower.contains("fetch first")
        || lower.contains("tip of your current branch is behind")
}

/// Union of two issue org files. Headings are keyed by `:ID:`. The same id
/// is one heading whose properties and body keep every line either side
/// had. A heading with no id stays if its text is not already there.
#[must_use]
pub(crate) fn union_org(ours: &str, theirs: &str) -> String {
    let (pre_ours, ours_heads) = parse_org(ours);
    let (pre_theirs, theirs_heads) = parse_org(theirs);
    let mut preamble = if pre_ours == pre_theirs {
        pre_ours
    } else {
        union_lines(&pre_ours, &pre_theirs)
    };
    if !preamble.is_empty() && !preamble.ends_with('\n') {
        preamble.push('\n');
    }
    let mut seen_ids = BTreeSet::new();
    let mut seen_anon = BTreeSet::new();
    let mut headings = Vec::new();
    for heading in ours_heads.iter().chain(theirs_heads.iter()) {
        if let Some(id) = heading_id(&heading.raw) {
            if !seen_ids.insert(id.clone()) {
                if let Some(pos) = headings
                    .iter()
                    .position(|have: &String| heading_id(have).as_deref() == Some(id.as_str()))
                {
                    headings[pos] = union_heading(&headings[pos], &heading.raw);
                }
                continue;
            }
        } else if !seen_anon.insert(heading.raw.trim().to_string()) {
            continue;
        }
        headings.push(heading.raw.clone());
    }
    let mut out = preamble;
    for heading in headings {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(heading.trim_end());
        out.push('\n');
    }
    out
}

struct OrgHeading {
    raw: String,
}

fn parse_org(text: &str) -> (String, Vec<OrgHeading>) {
    let mut preamble = String::new();
    let mut headings = Vec::new();
    let mut current: Option<String> = None;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if is_top_heading(bare) {
            if let Some(raw) = current.take() {
                headings.push(OrgHeading { raw });
            }
            current = Some(String::new());
        }
        if let Some(raw) = current.as_mut() {
            raw.push_str(line);
        } else {
            preamble.push_str(line);
        }
    }
    if let Some(raw) = current {
        headings.push(OrgHeading { raw });
    }
    (preamble, headings)
}

fn is_top_heading(line: &str) -> bool {
    line.starts_with("* ") && !line.starts_with("**")
}

fn heading_id(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(":ID:") {
            let id = rest.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn union_lines(ours: &str, theirs: &str) -> String {
    let mut out = ours.to_string();
    for line in theirs.lines() {
        if line.is_empty() {
            continue;
        }
        if !ours.lines().any(|have| have == line) {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn union_heading(ours: &str, theirs: &str) -> String {
    if ours.trim() == theirs.trim() {
        return ours.trim_end().to_string() + "\n";
    }
    let (headline, mut props, mut body) = split_heading(ours);
    let (_, props_b, body_b) = split_heading(theirs);
    for (key, value) in props_b {
        if !props
            .iter()
            .any(|(have_key, have_value)| have_key == &key && have_value == &value)
        {
            props.push((key, value));
        }
    }
    for line in body_b {
        if !body.iter().any(|have| have == &line) {
            body.push(line);
        }
    }
    let mut out = headline;
    out.push('\n');
    if !props.is_empty() {
        out.push_str(":PROPERTIES:\n");
        for (key, value) in props {
            out.push(':');
            out.push_str(&key);
            out.push_str(": ");
            out.push_str(&value);
            out.push('\n');
        }
        out.push_str(":END:\n");
    }
    for line in body {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn split_heading(text: &str) -> (String, Vec<(String, String)>, Vec<String>) {
    let mut lines = text.lines();
    let headline = lines.next().unwrap_or("").trim_end().to_string();
    let mut props = Vec::new();
    let mut body = Vec::new();
    let mut in_props = false;
    for line in lines {
        let trimmed = line.trim();
        if !in_props && trimmed == ":PROPERTIES:" {
            in_props = true;
            continue;
        }
        if in_props && trimmed == ":END:" {
            in_props = false;
            continue;
        }
        if in_props {
            if let Some(rest) = trimmed.strip_prefix(':') {
                if let Some((key, value)) = rest.split_once(':') {
                    let key = key.trim();
                    if !key.is_empty() {
                        props.push((key.to_string(), value.trim().to_string()));
                    }
                    continue;
                }
            }
        }
        body.push(line.trim_end().to_string());
    }
    (headline, props, body)
}

fn primary_push(dir: &Path) -> String {
    if upstream_set(dir) {
        return "git push -q".to_string();
    }
    match branch_name(dir) {
        Some(branch) => format!("git push -q -u origin {}", crate::tools::sh_quote(&branch)),
        None => "git push -q".to_string(),
    }
}

fn push_once(dir: &Path) -> Output {
    if upstream_set(dir) {
        git(dir, &["push", "-q"])
    } else if let Some(branch) = branch_name(dir) {
        git(dir, &["push", "-q", "-u", "origin", &branch])
    } else {
        git(dir, &["push", "-q"])
    }
}

fn push_mirrors(dir: &Path) -> Result<(), String> {
    let Some(up) = crate::tracker_upstream(dir) else {
        return Ok(());
    };
    let mirrors = crate::tracker_mirrors(dir, &up).unwrap_or_default();
    for (remote, branch) in mirrors {
        let spec = format!("HEAD:refs/heads/{branch}");
        let out = git(dir, &["push", "-q", &remote, &spec]);
        if !out.status.success() {
            return Err(format!(
                "mirror {remote} refused: {}",
                first_line(&output_text(&out))
            ));
        }
    }
    Ok(())
}

fn integrate(dir: &Path) -> Result<(), String> {
    let fetched = git(dir, &["fetch", "-q", "origin"]);
    if !fetched.status.success() {
        let _ = abort_rebase(dir);
        return Err(format!(
            "fetch failed: {}",
            first_line(&output_text(&fetched))
        ));
    }
    let onto = rebase_target(dir);
    let rebased = rebase_onto(dir, &onto);
    if rebased.is_err() {
        let _ = abort_rebase(dir);
    }
    rebased
}

fn rebase_target(dir: &Path) -> String {
    if let Some(up) = crate::tracker_upstream(dir) {
        if up != "origin/HEAD" && up.contains('/') {
            return up;
        }
    }
    match branch_name(dir) {
        Some(branch) => format!("origin/{branch}"),
        None => "origin/HEAD".to_string(),
    }
}

fn rebase_onto(dir: &Path, onto: &str) -> Result<(), String> {
    let started = git(dir, &["rebase", "--autostash", onto]);
    if started.status.success() {
        return Ok(());
    }
    let mut last = first_line(&output_text(&started));
    for _ in 0..12 {
        if !rebase_in_progress(dir) {
            if has_unmerged(dir) {
                resolve_unmerged(dir)?;
            }
            return Ok(());
        }
        if !has_unmerged(dir) {
            return Err(last);
        }
        resolve_unmerged(dir)?;
        let cont = git(dir, &["rebase", "--continue"]);
        if cont.status.success() {
            if has_unmerged(dir) {
                resolve_unmerged(dir)?;
            }
            return Ok(());
        }
        last = first_line(&output_text(&cont));
        let lower = last.to_ascii_lowercase();
        if lower.contains("no changes")
            || lower.contains("nothing to commit")
            || lower.contains("empty")
        {
            let skip = git(dir, &["rebase", "--skip"]);
            if skip.status.success() {
                return Ok(());
            }
            last = first_line(&output_text(&skip));
        }
    }
    Err(last)
}

fn resolve_unmerged(dir: &Path) -> Result<(), String> {
    let files = unmerged(dir);
    if files.is_empty() {
        return Ok(());
    }
    for file in &files {
        if !file.ends_with(".org") {
            return Err(format!(
                "rebase stopped on {file}, which is not an issue org file"
            ));
        }
        let ours = stage(dir, 2, file);
        let theirs = stage(dir, 3, file);
        let merged = match (ours, theirs) {
            (Some(a), Some(b)) => union_org(&a, &b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => {
                return Err(format!(
                    "rebase stopped on {file} with nothing on either side"
                ));
            }
        };
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, merged).map_err(|e| e.to_string())?;
        let added = git(dir, &["add", "--", file]);
        if !added.status.success() {
            return Err(format!(
                "could not mark {file} resolved: {}",
                first_line(&output_text(&added))
            ));
        }
    }
    Ok(())
}

fn stage(dir: &Path, n: u8, file: &str) -> Option<String> {
    let spec = format!(":{n}:{file}");
    let out = git(dir, &["show", &spec]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn unmerged(dir: &Path) -> Vec<String> {
    let out = git(dir, &["diff", "--name-only", "-z", "--diff-filter=U"]);
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_string)
        .collect()
}

fn has_unmerged(dir: &Path) -> bool {
    !unmerged(dir).is_empty()
}

fn rebase_in_progress(dir: &Path) -> bool {
    for name in ["rebase-merge", "rebase-apply"] {
        let out = git(dir, &["rev-parse", "--git-path", name]);
        if !out.status.success() {
            continue;
        }
        let rel = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if rel.is_empty() {
            continue;
        }
        let path = Path::new(&rel);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            dir.join(path)
        };
        if path.exists() {
            return true;
        }
    }
    false
}

fn abort_rebase(dir: &Path) -> Result<(), String> {
    if !rebase_in_progress(dir) {
        return Ok(());
    }
    let out = git(dir, &["rebase", "--abort"]);
    if out.status.success() {
        Ok(())
    } else {
        Err(first_line(&output_text(&out)))
    }
}

fn upstream_set(dir: &Path) -> bool {
    git(
        dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .status
    .success()
}

fn branch_name(dir: &Path) -> Option<String> {
    let out = git(dir, &["symbolic-ref", "--short", "HEAD"]);
    out.status.success().then(|| {
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!name.is_empty()).then_some(name)
    })?
}

fn git(dir: &Path, args: &[&str]) -> Output {
    use std::os::unix::process::ExitStatusExt;
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|e| Output {
            status: std::process::ExitStatus::from_raw(1),
            stdout: Vec::new(),
            stderr: e.to_string().into_bytes(),
        })
}

fn output_text(out: &Output) -> String {
    let mut said = String::from_utf8_lossy(&out.stderr).into_owned();
    if said.trim().is_empty() {
        said = String::from_utf8_lossy(&out.stdout).into_owned();
    }
    said
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("push refused")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(id: &str, title: &str) -> String {
        format!("* TODO [#C] {title}\n:PROPERTIES:\n:ID:         {id}\n:END:\n")
    }

    #[test]
    fn a_union_keeps_both_headings_keyed_by_id() {
        let preamble = "#+TITLE: probe\n#+TODO: TODO | DONE\n\n";
        let seed = heading("seed", "Seed");
        let ours = format!("{preamble}{seed}{}", heading("from-a", "From A"));
        let theirs = format!("{preamble}{seed}{}", heading("from-b", "From B"));
        let union = union_org(&ours, &theirs);
        assert!(union.contains(":ID:         seed"), "{union}");
        assert!(union.contains(":ID:         from-a"), "{union}");
        assert!(union.contains(":ID:         from-b"), "{union}");
        assert_eq!(union.matches("* TODO").count(), 3, "{union}");
        let again = union_org(&union, &ours);
        assert_eq!(again.matches("* TODO").count(), 3, "{again}");
    }

    #[test]
    fn the_same_id_keeps_lines_from_both_sides() {
        let ours = "\
* STARTED [#C] Seed
:PROPERTIES:
:ID:         seed
:CLAIMED_BY: a@box
:END:
note from a
";
        let theirs = "\
* STARTED [#C] Seed
:PROPERTIES:
:ID:         seed
:CLAIMED_BY: b@box
:END:
note from b
";
        let union = union_org(ours, theirs);
        assert_eq!(union.matches("* STARTED").count(), 1, "{union}");
        assert!(union.contains("a@box"), "{union}");
        assert!(union.contains("b@box"), "{union}");
        assert!(union.contains("note from a"), "{union}");
        assert!(union.contains("note from b"), "{union}");
    }

    #[test]
    fn a_non_fast_forward_is_the_rejection_that_retries() {
        assert!(rejected(
            " ! [rejected] main -> main (non-fast-forward)\nfatal: failed to push some refs"
        ));
        assert!(rejected(
            "Updates were rejected because the tip of your current branch is behind"
        ));
        assert!(!rejected(
            "fatal: 'origin' does not appear to be a git repository"
        ));
    }

    #[test]
    fn two_clones_race_and_both_headings_land() {
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let run = |cwd: &Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} in {}: {}",
                cwd.display(),
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(
            dir.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        run(&origin, &["config", "receive.denyNonFastForwards", "true"]);
        run(
            dir.path(),
            &["clone", "-q", origin.to_str().unwrap(), a.to_str().unwrap()],
        );
        for (key, value) in [
            ("user.email", "seat@example.invalid"),
            ("user.name", "seat"),
            ("core.hooksPath", "/dev/null"),
            ("commit.gpgsign", "false"),
        ] {
            run(&a, &["config", key, value]);
        }
        let preamble = "#+TITLE: probe\n\n";
        let seed = heading("seed", "Seed");
        std::fs::write(a.join("issues.org"), format!("{preamble}{seed}")).unwrap();
        run(&a, &["add", "issues.org"]);
        run(&a, &["commit", "-q", "-m", "seed"]);
        run(&a, &["push", "-q", "-u", "origin", "HEAD"]);
        run(
            dir.path(),
            &["clone", "-q", origin.to_str().unwrap(), b.to_str().unwrap()],
        );
        for (key, value) in [
            ("user.email", "seat@example.invalid"),
            ("user.name", "seat"),
            ("core.hooksPath", "/dev/null"),
            ("commit.gpgsign", "false"),
        ] {
            run(&b, &["config", key, value]);
        }
        std::fs::write(
            a.join("issues.org"),
            format!("{preamble}{seed}{}", heading("from-a", "From A")),
        )
        .unwrap();
        std::fs::write(
            b.join("issues.org"),
            format!("{preamble}{seed}{}", heading("from-b", "From B")),
        )
        .unwrap();
        run(&a, &["add", "issues.org"]);
        run(&a, &["commit", "-q", "-m", "from a"]);
        run(&b, &["add", "issues.org"]);
        run(&b, &["commit", "-q", "-m", "from b"]);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let left = {
            let barrier = barrier.clone();
            let a = a.clone();
            std::thread::spawn(move || {
                barrier.wait();
                push_checkout(&a)
            })
        };
        let right = std::thread::spawn(move || {
            barrier.wait();
            push_checkout(&b)
        });
        let left = left.join().unwrap();
        let right = right.join().unwrap();
        assert!(left.is_ok(), "clone a: {left:?}");
        assert!(right.is_ok(), "clone b: {right:?}");
        let shown = std::process::Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["show", "HEAD:issues.org"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&shown.stdout);
        assert!(text.contains(":ID:         from-a"), "{text}");
        assert!(text.contains(":ID:         from-b"), "{text}");
        assert!(text.contains(":ID:         seed"), "{text}");
    }
}
