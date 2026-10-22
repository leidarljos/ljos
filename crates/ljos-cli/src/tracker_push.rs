//! Push a tracker checkout without losing another seat's edits.
//!
//! Seats share one tracker. A later push is rejected as non-fast-forward.
//! The push fetches and rebases with autostash. When that rebase stops on
//! a conflict, issue org files are merged field by field. Atom logs are
//! re-exported by the `ljos-atoms` driver, which writes this machine's log
//! again rather than keeping either side. A file that still has two
//! headings with one `:ID:` is folded. Nothing is force-pushed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;

const ATTEMPTS: usize = 6;
const ATTR_ORG: &str = "*.org merge=ljos-org";
const ATTR_ATOMS: &str = ".ljos/atoms/* merge=ljos-atoms";
const DRIVER_KEY: &str = "merge.ljos-atoms.driver";
const DRIVER_NAME_KEY: &str = "merge.ljos-atoms.name";
const DRIVER_NAME: &str = "re-export local atoms";
const ORG_DRIVER_KEY: &str = "merge.ljos-org.driver";
const ORG_DRIVER_NAME_KEY: &str = "merge.ljos-org.name";
const ORG_DRIVER_NAME: &str = "field-merge issue org files";
const ATTR_COMMIT: &str = "chore(tracker): field-merge org files and re-export atoms on merge";
const FOLD_COMMIT: &str = "chore(tracker): fold duplicate issue headings";

/// Commit `dir`'s branch and the headings already on the remote.
///
/// Merge attributes are written and, when `dir` is already a checkout,
/// committed before the push. A branch with no upstream is pushed with
/// `-u origin <branch>`. A non-fast-forward fetch is rebased with
/// `--autostash`. A rebase that stops is aborted and merged. Org conflicts
/// take the per-issue field merge. This machine's atom logs are
/// re-exported. Duplicate `:ID:` headings are folded afterwards.
///
/// # Errors
///
/// The push still failed after a few attempts, or a conflict was not an
/// org file or this machine's atoms log.
pub(crate) fn push_checkout(start: &Path) -> Result<(), String> {
    let owned = repo_root(start);
    let dir = owned.as_path();
    prepare_tracker(dir, false, true)?;
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
        integrate(dir).inspect_err(|e| last = e.clone())?;
    }
    Err(format!(
        "{last}; still not a fast-forward after {ATTEMPTS} attempts"
    ))
}

/// Fetch, rebase with the org field merge, and push. Never force.
///
/// A failure leaves the local commits where they are. The next call tries
/// again. `ATTEMPTS` bounds the retries.
///
/// # Errors
///
/// The remote still does not have the commits, or an issue file could not
/// be merged.
pub(crate) fn publish(start: &Path) -> Result<(), String> {
    let owned = repo_root(start);
    let dir = owned.as_path();
    prepare_tracker(dir, false, true)?;
    let mut last = String::from("push refused");
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(
                200 * u64::try_from(attempt).unwrap_or(1),
            ));
        }
        if let Err(e) = integrate(dir) {
            last = e;
            if last.starts_with("fetch failed") {
                continue;
            }
            let _held = crate::tracker_commit_lock(dir);
            let _ = abort_rebase(dir);
            let _ = git(dir, &["merge", "--abort"]);
            return Err(last);
        }
        let out = push_once(dir);
        if out.status.success() {
            return push_mirrors(dir);
        }
        let said = output_text(&out);
        last = first_line(&said);
        if rejected(&said) {
            continue;
        }
    }
    Err(format!(
        "{last}; still not pushed after {ATTEMPTS} attempts"
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
    // Never `--force`. A rejected update is rebased and tried again.
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
    // The replay rewrites the index and the working tree, so it waits for
    // a seat's commit in flight and no commit starts until it is done. The
    // fetch, which may wait on the network, is outside.
    let _held = crate::tracker_commit_lock(dir);
    if !fetched.status.success() {
        let _ = abort_rebase(dir);
        return Err(format!(
            "fetch failed: {}",
            first_line(&output_text(&fetched))
        ));
    }
    let onto = rebase_target(dir);
    if let Err(e) = rebase_onto(dir, &onto) {
        if rebase_in_progress(dir) || has_unmerged(dir) {
            clear_stopped(dir)?;
            merge_onto(dir, &onto)?;
        } else {
            let _ = abort_rebase(dir);
            return Err(e);
        }
    }
    fold_orgs(dir)?;
    refuse_markers(dir)
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
                return Ok(());
            }
            // The rebase command failed and left nothing to finish. That is
            // a refusal, not a clean replay: a missing onto used to look
            // like success and the push then had nothing new to send.
            return Err(last);
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
        if file.ends_with(".org") {
            resolve_org(dir, file)?;
        } else if file == ".gitattributes" {
            resolve_attributes(dir)?;
        } else if is_this_host_atoms(file) {
            resolve_atoms(dir, file)?;
        } else {
            return Err(format!(
                "rebase stopped on {file}, which is not an issue org file"
            ));
        }
    }
    Ok(())
}

fn resolve_org(dir: &Path, file: &str) -> Result<(), String> {
    let base = stage(dir, 1, file).unwrap_or_default();
    let ours = stage(dir, 2, file);
    let theirs = stage(dir, 3, file);
    let merged = match (ours, theirs) {
        (Some(a), Some(b)) => field_or_union(&base, &a, &b, file)?,
        (Some(a), None) | (None, Some(a)) => fold_org_text(&a),
        (None, None) => {
            return Err(format!(
                "rebase stopped on {file} with nothing on either side"
            ));
        }
    };
    if merged.contains("<<<<<<<") {
        return Err(format!("{file} still has conflict markers"));
    }
    write_resolved(dir, file, merged.as_bytes())
}

/// An issue board merges field by field. A file that is not one still
/// unions, so a note outside the tracker is not refused.
fn field_or_union(base: &str, ours: &str, theirs: &str, file: &str) -> Result<String, String> {
    match field_merge(base, ours, theirs) {
        Ok(text) => Ok(text),
        Err(err) if looks_like_issue(ours) || looks_like_issue(theirs) => {
            Err(format!("{file}: {err}"))
        }
        Err(_) => Ok(fold_org_text(&union_org(ours, theirs))),
    }
}

fn looks_like_issue(text: &str) -> bool {
    text.lines().any(|line| line.contains(":ID:"))
}

fn field_merge(base: &str, ours: &str, theirs: &str) -> Result<String, String> {
    let merged = vissue_core::merge::merge_tracker("tracker", base, ours, theirs)
        .map_err(|e| e.to_string())?;
    if merged.text.contains("<<<<<<<") {
        return Err("merged org still has conflict markers".into());
    }
    Ok(merged.text)
}

/// git's `ljos-org` merge driver. `ours` is `%A` and is replaced.
pub(crate) fn merge_org_files(base: &Path, ours: &Path, theirs: &Path) -> Result<(), String> {
    let base_txt = std::fs::read_to_string(base).unwrap_or_default();
    let ours_txt = std::fs::read_to_string(ours).map_err(|e| e.to_string())?;
    let theirs_txt = std::fs::read_to_string(theirs).map_err(|e| e.to_string())?;
    let merged = field_or_union(
        &base_txt,
        &ours_txt,
        &theirs_txt,
        &ours.display().to_string(),
    )?;
    if merged.contains("<<<<<<<") {
        return Err("merged org still has conflict markers".into());
    }
    std::fs::write(ours, merged).map_err(|e| e.to_string())
}

fn resolve_attributes(dir: &Path) -> Result<(), String> {
    let ours = stage(dir, 2, ".gitattributes").unwrap_or_default();
    let theirs = stage(dir, 3, ".gitattributes").unwrap_or_default();
    let mut merged = if ours.is_empty() {
        theirs
    } else if theirs.is_empty() {
        ours
    } else {
        union_lines(&ours, &theirs)
    };
    if !merged.is_empty() && !merged.ends_with('\n') {
        merged.push('\n');
    }
    for line in [ATTR_ORG, ATTR_ATOMS] {
        if !attr_present(&merged, line) {
            merged.push_str(line);
            merged.push('\n');
        }
    }
    write_resolved(dir, ".gitattributes", merged.as_bytes())
}

fn resolve_atoms(dir: &Path, file: &str) -> Result<(), String> {
    let bytes = reexport_local(dir, file)?;
    if bytes.windows(7).any(|w| w == b"<<<<<<<") {
        return Err(format!("{file} still has conflict markers"));
    }
    crate::sync::write_beneath(dir, file, &bytes).map_err(|e| format!("atoms merge: {e}"))?;
    mark_resolved(dir, file)
}

fn write_resolved(dir: &Path, file: &str, bytes: &[u8]) -> Result<(), String> {
    let path = dir.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    mark_resolved(dir, file)
}

fn mark_resolved(dir: &Path, file: &str) -> Result<(), String> {
    let added = git(dir, &["add", "--", file]);
    if added.status.success() {
        Ok(())
    } else {
        Err(format!(
            "could not mark {file} resolved: {}",
            first_line(&output_text(&added))
        ))
    }
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

/// The checkout that holds `start`. Commands below name paths from the
/// root; a subdirectory makes `git diff` and `git add` disagree about them.
pub(crate) fn repo_root(start: &Path) -> PathBuf {
    let out = git(start, &["rev-parse", "--show-toplevel"]);
    if out.status.success() {
        let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if path.is_dir() {
            return path;
        }
    }
    start.to_path_buf()
}

fn git(dir: &Path, args: &[&str]) -> Output {
    use std::os::unix::process::ExitStatusExt;
    let mut cmd = crate::git_at(dir);
    cmd.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .stdin(std::process::Stdio::null());
    crate::git_hooks::hook_child_env(&mut cmd);
    cmd.output().unwrap_or_else(|e| Output {
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

/// Write the tracker's merge attributes and register the atoms driver.
///
/// `init` makes a repository when `root` is not one yet, which is how
/// onboard and the tracker step start a new tracker. A sync or a push
/// passes `init` false and leaves a directory that is not a checkout
/// untouched. `commit` records `.gitattributes` when it changed. Running
/// this again changes nothing once both lines and the driver are in place.
///
/// # Errors
///
/// `git init` failed, the attributes could not be written, or the
/// attributes commit was refused.
pub(crate) fn prepare_tracker(root: &Path, init: bool, commit: bool) -> Result<(), String> {
    // Before any write. A test whose tracker root falls back to the crate
    // directory used to commit `.gitattributes` into the ljos checkout.
    crate::refuse_ljos_checkout(root)?;
    if !is_work_tree(root) {
        if !init {
            return Ok(());
        }
        init_repo(root)?;
    }
    let _held = crate::tracker_commit_lock(root);
    write_attributes(root)?;
    register_driver(root)?;
    if commit {
        commit_attributes(root)?;
    }
    Ok(())
}

/// git's `ljos-atoms` merge driver. `current` is `%A` and `rel` is `%P`.
/// The file becomes a fresh export of this machine's atoms.
pub(crate) fn merge_atoms_driver(current: &Path, rel: &str) -> Result<(), String> {
    let root = work_tree_of(current).or_else(|_| work_tree_cwd())?;
    let bytes = reexport_local(&root, rel.trim())?;
    // git's cwd is the work tree and `%A` is relative (`.merge_file_XXXX`).
    // `strip_prefix` on that relative path never matches the absolute root,
    // so the driver errors and the merge aborts. Resolve it against the cwd
    // first. Opening the result by name would follow a parent swapped for a
    // symlink; the beneath check stays, and the write goes through the
    // directory fd, the same way the rebase path does.
    let resolved = if current.is_relative() {
        std::env::current_dir()
            .map_err(|e| format!("atoms merge: cwd: {e}"))?
            .join(current)
    } else {
        current.to_path_buf()
    };
    let rel_current = resolved.strip_prefix(&root).map_err(|_| {
        format!(
            "atoms merge: {} is outside the work tree",
            resolved.display()
        )
    })?;
    let rel_current = rel_current
        .to_str()
        .ok_or_else(|| format!("atoms merge: {} is not a path", resolved.display()))?;
    crate::sync::write_beneath(&root, rel_current, &bytes).map_err(|e| format!("atoms merge: {e}"))
}

/// One org file, after a union. Two headings that share a `:ID:` become
/// one heading that keeps both bodies. Text with no duplicate id is
/// returned unchanged.
#[must_use]
pub(crate) fn fold_org_text(text: &str) -> String {
    let (_, headings) = parse_org(text);
    let mut seen = BTreeSet::new();
    let mut duplicated = false;
    for heading in &headings {
        if let Some(id) = heading_id(&heading.raw) {
            if !seen.insert(id) {
                duplicated = true;
                break;
            }
        }
    }
    if !duplicated {
        return text.to_string();
    }
    union_org(text, "")
}

fn init_repo(root: &Path) -> Result<(), String> {
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let made = git(root, &["init", "-q", "-b", "main"]);
    if made.status.success() {
        return Ok(());
    }
    let again = git(root, &["init", "-q"]);
    if again.status.success() {
        Ok(())
    } else {
        Err(format!("git init: {}", first_line(&output_text(&again))))
    }
}

fn is_work_tree(root: &Path) -> bool {
    if !root.is_dir() {
        return false;
    }
    let Some(top) = checkout_root(root) else {
        return false;
    };
    let want = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let have = std::fs::canonicalize(&top).unwrap_or(top);
    have == want
}

fn checkout_root(root: &Path) -> Option<PathBuf> {
    let out = git(root, &["rev-parse", "--show-toplevel"]);
    if !out.status.success() {
        return None;
    }
    let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if top.is_empty() {
        return None;
    }
    Some(PathBuf::from(top))
}

fn write_attributes(root: &Path) -> Result<(), String> {
    let path = root.join(".gitattributes");
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines = Vec::new();
    let mut saw_org = false;
    let mut saw_atoms = false;
    for line in have.lines() {
        let trimmed = line.trim();
        if org_merge_line(trimmed) {
            if !saw_org {
                lines.push(ATTR_ORG.to_string());
                saw_org = true;
            }
            continue;
        }
        if attr_present(line, ATTR_ATOMS) {
            saw_atoms = true;
        }
        lines.push(line.to_string());
    }
    if !saw_org {
        lines.push(ATTR_ORG.to_string());
    }
    if !saw_atoms {
        lines.push(ATTR_ATOMS.to_string());
    }
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if out == have {
        return Ok(());
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

fn org_merge_line(trimmed: &str) -> bool {
    let mut parts = trimmed.split_whitespace();
    parts.next() == Some("*.org") && parts.any(|word| word.starts_with("merge="))
}

fn attr_present(text: &str, line: &str) -> bool {
    let Some((pattern, attr)) = line.split_once(' ') else {
        return false;
    };
    text.lines().any(|have| {
        let have = have.trim();
        if have.is_empty() || have.starts_with('#') {
            return false;
        }
        let mut parts = have.split_whitespace();
        parts.next() == Some(pattern) && parts.any(|word| word == attr)
    })
}

fn driver_command() -> String {
    match std::env::var("LJOS_ATOMS_DRIVER") {
        Ok(over) if !over.trim().is_empty() => over,
        _ => "ljos merge-atoms %A %P".to_string(),
    }
}

fn org_driver_command() -> String {
    if let Ok(over) = std::env::var("LJOS_ORG_DRIVER") {
        if !over.trim().is_empty() {
            return over;
        }
    }
    match std::env::current_exe() {
        Ok(path) if path.file_name().and_then(|n| n.to_str()) == Some("ljos") => format!(
            "{} merge-org %O %A %B",
            crate::tools::sh_quote(&path.display().to_string())
        ),
        _ => "ljos merge-org %O %A %B".to_string(),
    }
}

fn register_driver(root: &Path) -> Result<(), String> {
    let want = driver_command();
    if local_config(root, DRIVER_KEY).as_deref() != Some(want.as_str()) {
        let set = git(root, &["config", "--local", DRIVER_KEY, &want]);
        if !set.status.success() {
            return Err(format!(
                "could not register the atoms merge driver: {}",
                first_line(&output_text(&set))
            ));
        }
    }
    if local_config(root, DRIVER_NAME_KEY).as_deref() != Some(DRIVER_NAME) {
        let set = git(root, &["config", "--local", DRIVER_NAME_KEY, DRIVER_NAME]);
        if !set.status.success() {
            return Err(format!(
                "could not name the atoms merge driver: {}",
                first_line(&output_text(&set))
            ));
        }
    }
    let org = org_driver_command();
    if local_config(root, ORG_DRIVER_KEY).as_deref() != Some(org.as_str()) {
        let set = git(root, &["config", "--local", ORG_DRIVER_KEY, &org]);
        if !set.status.success() {
            return Err(format!(
                "could not register the org merge driver: {}",
                first_line(&output_text(&set))
            ));
        }
    }
    if local_config(root, ORG_DRIVER_NAME_KEY).as_deref() != Some(ORG_DRIVER_NAME) {
        let set = git(
            root,
            &["config", "--local", ORG_DRIVER_NAME_KEY, ORG_DRIVER_NAME],
        );
        if !set.status.success() {
            return Err(format!(
                "could not name the org merge driver: {}",
                first_line(&output_text(&set))
            ));
        }
    }
    Ok(())
}

fn local_config(dir: &Path, key: &str) -> Option<String> {
    let out = git(dir, &["config", "--local", "--get", key]);
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn commit_attributes(root: &Path) -> Result<(), String> {
    let added = git(root, &["add", "--", ".gitattributes"]);
    if !added.status.success() {
        return Err(format!(
            "could not stage .gitattributes: {}",
            first_line(&output_text(&added))
        ));
    }
    let staged = git(
        root,
        &["diff", "--cached", "--quiet", "--", ".gitattributes"],
    );
    if staged.status.success() {
        return Ok(());
    }
    let out = git(
        root,
        &[
            "commit",
            "-q",
            "--only",
            "-m",
            ATTR_COMMIT,
            "--",
            ".gitattributes",
        ],
    );
    if out.status.success() {
        return Ok(());
    }
    let said = output_text(&out);
    let lower = said.to_ascii_lowercase();
    if lower.contains("nothing to commit") || lower.contains("no changes") {
        return Ok(());
    }
    Err(format!(
        "could not commit .gitattributes: {}",
        first_line(&said)
    ))
}

fn work_tree_of(path: &Path) -> Result<PathBuf, String> {
    let start = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    toplevel(start)
}

fn work_tree_cwd() -> Result<PathBuf, String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("atoms merge: {}", first_line(&output_text(&out))));
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        return Err("atoms merge: not a git work tree".into());
    }
    Ok(PathBuf::from(path))
}

fn toplevel(start: &Path) -> Result<PathBuf, String> {
    let out = git(start, &["rev-parse", "--show-toplevel"]);
    if !out.status.success() {
        return Err(format!("atoms merge: {}", first_line(&output_text(&out))));
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        return Err("atoms merge: not a git work tree".into());
    }
    Ok(PathBuf::from(path))
}

/// A fresh export of this machine's log at `rel`. Another machine's file
/// is refused: re-exporting it would publish our atoms under their name.
/// No scope returns before the pack is asked. An unchanged export writes
/// nothing, so the file is read with no symlink at any component: the
/// tracker can hold a link there to a file outside it.
fn reexport_local(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    if !is_this_host_atoms(rel) {
        return Err(format!(
            "atoms merge: {rel} belongs to another machine; this driver re-exports only {}",
            crate::sync::host()
        ));
    }
    let scope = crate::sync::scope_of_repo(root).map_err(|e| e.to_string())?;
    let Some(scope) = scope else {
        return Err("atoms merge: this tracker names no scope".into());
    };
    let Some(scope_name) = scope_for_this_host_atoms(rel, &scope) else {
        return Err(format!(
            "atoms merge: {rel} names a scope this tracker does not configure"
        ));
    };
    let client = crate::pack().map_err(|e| e.to_string())?;
    let atoms = crate::atoms_lean(&client, &client.workspace()).map_err(|e| e.to_string())?;
    crate::sync::export_named(root, &scope, &scope_name, &atoms).map_err(|e| e.to_string())?;
    let name =
        atoms_file_name(rel).ok_or_else(|| format!("atoms merge: {rel} has no file name"))?;
    let fresh = format!(".ljos/atoms/{name}");
    crate::sync::read_beneath(root, &fresh)
        .map_err(|e| format!("atoms merge: {e}"))?
        .ok_or_else(|| format!("atoms merge: {fresh} was not written"))
}

fn atoms_file_name(rel: &str) -> Option<&str> {
    Path::new(rel.trim())
        .file_name()
        .and_then(|name| name.to_str())
}

fn is_this_host_atoms(rel: &str) -> bool {
    let Some(name) = atoms_file_name(rel) else {
        return false;
    };
    let host = crate::sync::host();
    if name == format!("{host}.jsonl.age") || name == format!("{host}.digest") {
        return true;
    }
    let Some(rest) = name.strip_prefix(&format!("{host}+")) else {
        return false;
    };
    !rest.is_empty() && (rest.ends_with(".jsonl.age") || rest.ends_with(".digest"))
}

fn scope_for_this_host_atoms(rel: &str, scope: &crate::sync::Scope) -> Option<String> {
    let name = atoms_file_name(rel)?;
    let host = crate::sync::host();
    if name == format!("{host}.jsonl.age") || name == format!("{host}.digest") {
        return Some(scope.name.clone());
    }
    scope
        .scopes
        .keys()
        .find(|scope_name| {
            let suffix_age = format!("{host}+{scope_name}.jsonl.age");
            let suffix_digest = format!("{host}+{scope_name}.digest");
            name == suffix_age || name == suffix_digest
        })
        .cloned()
}

fn merge_onto(dir: &Path, onto: &str) -> Result<(), String> {
    crate::refuse_ljos_checkout(dir)?;
    let started = git(dir, &["merge", "-q", "--no-commit", "--no-edit", onto]);
    let stopped = merge_in_progress(dir) || has_unmerged(dir);
    if !started.status.success() && !stopped {
        return Err(first_line(&output_text(&started)));
    }
    if !stopped {
        return Ok(());
    }
    let resolved = (|| {
        if has_unmerged(dir) {
            resolve_unmerged(dir)?;
        }
        refuse_markers(dir)
    })();
    if let Err(e) = resolved {
        let _ = git(dir, &["merge", "--abort"]);
        return Err(e);
    }
    let cont = git(dir, &["commit", "-q", "--no-edit"]);
    if cont.status.success() {
        return Ok(());
    }
    let said = output_text(&cont);
    let lower = said.to_ascii_lowercase();
    if lower.contains("nothing to commit") || lower.contains("no changes") {
        return Ok(());
    }
    let _ = git(dir, &["merge", "--abort"]);
    Err(first_line(&said))
}

fn fold_orgs(dir: &Path) -> Result<(), String> {
    crate::refuse_ljos_checkout(dir)?;
    let mut changed = Vec::new();
    for file in tracked(dir, "*.org") {
        let path = dir.join(&file);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let folded = fold_org_text(text);
        if folded == text {
            continue;
        }
        if folded.contains("<<<<<<<") {
            return Err(format!("{file} still has conflict markers"));
        }
        std::fs::write(&path, &folded).map_err(|e| e.to_string())?;
        mark_resolved(dir, &file)?;
        changed.push(file);
    }
    if changed.is_empty() || rebase_in_progress(dir) || merge_in_progress(dir) {
        return Ok(());
    }
    let mut args = vec!["commit", "-q", "--only", "-m", FOLD_COMMIT, "--"];
    args.extend(changed.iter().map(String::as_str));
    let out = git(dir, &args);
    if out.status.success() {
        return Ok(());
    }
    let said = output_text(&out);
    let lower = said.to_ascii_lowercase();
    if lower.contains("nothing to commit") || lower.contains("no changes") {
        return Ok(());
    }
    Err(format!(
        "could not fold duplicate headings: {}",
        first_line(&said)
    ))
}

fn refuse_markers(dir: &Path) -> Result<(), String> {
    let mut files = tracked(dir, "*.org");
    files.extend(tracked(dir, ".ljos/atoms"));
    for file in files {
        let read = if file.starts_with(".ljos/") {
            crate::sync::read_beneath(dir, &file).map_err(|e| e.to_string())
        } else {
            match std::fs::read(dir.join(&file)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            }
        };
        let Some(bytes) = read? else {
            continue;
        };
        if bytes.windows(7).any(|window| window == b"<<<<<<<") {
            return Err(format!("{file} still has conflict markers"));
        }
    }
    Ok(())
}

fn tracked(dir: &Path, spec: &str) -> Vec<String> {
    let out = git(dir, &["ls-files", "-z", "--", spec]);
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect()
}

fn merge_in_progress(dir: &Path) -> bool {
    git_path_exists(dir, "MERGE_HEAD")
}

fn clear_stopped(dir: &Path) -> Result<(), String> {
    if rebase_in_progress(dir) {
        return abort_rebase(dir);
    }
    if merge_in_progress(dir) || has_unmerged(dir) {
        let out = git(dir, &["merge", "--abort"]);
        if out.status.success() || !has_unmerged(dir) {
            return Ok(());
        }
        return Err(first_line(&output_text(&out)));
    }
    Ok(())
}

fn git_path_exists(dir: &Path, name: &str) -> bool {
    let out = git(dir, &["rev-parse", "--git-path", name]);
    if !out.status.success() {
        return false;
    }
    let rel = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if rel.is_empty() {
        return false;
    }
    let path = Path::new(&rel);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    };
    path.exists()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

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
    fn two_notes_on_one_issue_merge_by_field() {
        let head = "#+VISSUE: 1\n#+TODO: TODO STARTED | DONE\n\n";
        let issue = |log: &str| {
            format!("* TODO [#A] Work\n:PROPERTIES:\n:ID:       acme-1a2b\n:END:\n:LOGBOOK:\n{log}:END:\n")
        };
        let n0 = "- Note: \"started\" [2026-09-20 Sun 10:00]\n";
        let na = "- Note: \"ours saw the build go green\" [2026-09-27 Sun 11:00]\n";
        let nb = "- Note: \"theirs filed the log\" [2026-09-27 Sun 12:00]\n";
        let merged = field_merge(
            &format!("{head}{}", issue(n0)),
            &format!("{head}{}", issue(&format!("{na}{n0}"))),
            &format!("{head}{}", issue(&format!("{nb}{n0}"))),
        )
        .unwrap();
        assert!(merged.contains("ours saw the build go green"), "{merged}");
        assert!(merged.contains("theirs filed the log"), "{merged}");
        assert_eq!(merged.matches("* TODO").count(), 1, "{merged}");
        assert!(!merged.contains("<<<<<<<"), "{merged}");
    }

    #[test]
    fn the_push_script_does_not_force() {
        let dir = tempfile::tempdir().unwrap();
        let script = push_script(dir.path(), &[]);
        assert!(!script.contains("force"), "{script}");
        assert!(!script.contains(" -f "), "{script}");
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
            let out = crate::git_at(cwd)
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
        std::fs::write(
            a.join(".gitattributes"),
            "*.org merge=union\n.ljos/atoms/* merge=ljos-atoms\n",
        )
        .unwrap();
        run(&a, &["add", "issues.org", ".gitattributes"]);
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
        assert!(!text.contains("<<<<<<<"), "{text}");
    }

    #[test]
    fn duplicate_ids_fold_into_one_heading() {
        let text = "\
* TODO [#C] One
:PROPERTIES:
:ID:         same
:END:
body a
* TODO [#C] One
:PROPERTIES:
:ID:         same
:END:
body b
";
        let folded = fold_org_text(text);
        assert_eq!(folded.matches("* TODO").count(), 1, "{folded}");
        assert!(folded.contains("body a"), "{folded}");
        assert!(folded.contains("body b"), "{folded}");
        let plain = "* TODO [#C] Only\n:PROPERTIES:\n:ID:         only\n:END:\n";
        assert_eq!(fold_org_text(plain), plain);
    }

    #[test]
    fn prepare_writes_the_attributes_and_the_driver_once() {
        let _lock = crate::test_env_lock();
        let previous = std::env::var_os("LJOS_ATOMS_DRIVER");
        unsafe { std::env::remove_var("LJOS_ATOMS_DRIVER") };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .env("GIT_TERMINAL_PROMPT", "0")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        prepare_tracker(root, true, false).unwrap();
        run(&["config", "user.email", "seat@example.invalid"]);
        run(&["config", "user.name", "seat"]);
        run(&["config", "commit.gpgsign", "false"]);
        prepare_tracker(root, true, true).unwrap();
        prepare_tracker(root, false, true).unwrap();
        let attrs = std::fs::read_to_string(root.join(".gitattributes")).unwrap();
        assert_eq!(
            attrs
                .lines()
                .filter(|line| line.trim() == "*.org merge=ljos-org")
                .count(),
            1,
            "{attrs}"
        );
        assert_eq!(
            attrs
                .lines()
                .filter(|line| line.trim() == ".ljos/atoms/* merge=ljos-atoms")
                .count(),
            1,
            "{attrs}"
        );
        let driver = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["config", "--local", "--get", "merge.ljos-atoms.driver"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&driver.stdout).trim(),
            "ljos merge-atoms %A %P"
        );
        let name = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["config", "--local", "--get", "merge.ljos-atoms.name"])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&name.stdout).trim(),
            "re-export local atoms"
        );
        let count = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "1");
        unsafe {
            match previous {
                Some(value) => std::env::set_var("LJOS_ATOMS_DRIVER", value),
                None => std::env::remove_var("LJOS_ATOMS_DRIVER"),
            }
        }
    }

    #[test]
    fn prepare_tracker_inits_the_nested_tracker_not_the_enclosing_checkout() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("repo");
        let nested = outer.join("target/tracker-home");
        std::fs::create_dir_all(&outer).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let run_outer = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&outer)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        run_outer(&["init", "-q", "-b", "main"]);
        run_outer(&["config", "user.email", "seat@example.invalid"]);
        run_outer(&["config", "user.name", "seat"]);
        run_outer(&["config", "core.hooksPath", "/dev/null"]);
        let config_get = |key: &str| -> String {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&outer)
                .args(["config", "--local", "--get", key])
                .output()
                .unwrap();
            if out.status.success() {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                String::new()
            }
        };
        let before_exclude =
            std::fs::read_to_string(outer.join(".git/info/exclude")).unwrap_or_default();
        let before_driver = config_get("merge.ljos-atoms.driver");

        prepare_tracker(&nested, true, false).unwrap();
        let _held = crate::tracker_commit_lock(&nested);

        let top_nested = std::process::Command::new("git")
            .arg("-C")
            .arg(&nested)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .unwrap();
        assert!(
            top_nested.status.success(),
            "nested rev-parse failed: {}",
            String::from_utf8_lossy(&top_nested.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&top_nested.stdout).trim(),
            nested.display().to_string()
        );
        assert!(
            nested.join(".gitattributes").is_file(),
            "nested tracker owns attributes"
        );
        assert!(
            nested.join(crate::TRACKER_LOCK).is_file(),
            "nested tracker owns the commit lock"
        );
        assert!(
            !outer.join(crate::TRACKER_LOCK).exists(),
            "enclosing checkout gained no lock"
        );
        let after_exclude =
            std::fs::read_to_string(outer.join(".git/info/exclude")).unwrap_or_default();
        assert_eq!(
            after_exclude, before_exclude,
            "enclosing checkout exclude changed"
        );
        let after_driver = config_get("merge.ljos-atoms.driver");
        assert_eq!(
            after_driver, before_driver,
            "enclosing checkout gained a merge driver"
        );
    }

    #[test]
    fn prepare_tracker_does_not_retarget_to_exported_git_dir() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("dots.git");
        let home = dir.path().join("home");
        let tracker = dir.path().join("tracker");
        std::fs::create_dir_all(&home).unwrap();
        let init = std::process::Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&bare)
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "bare init: {}",
            String::from_utf8_lossy(&init.stderr)
        );

        let git_bare = |args: &[&str]| -> String {
            let out = std::process::Command::new("git")
                .arg("--git-dir")
                .arg(&bare)
                .args(args)
                .output()
                .unwrap();
            if out.status.success() {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            } else {
                String::new()
            }
        };

        let before_bare = git_bare(&["config", "--get", "core.bare"]);
        let before_worktree = git_bare(&["config", "--get", "core.worktree"]);
        let before_driver = git_bare(&["config", "--get", "merge.ljos-atoms.driver"]);
        let old_git_dir = std::env::var_os("GIT_DIR");
        let old_git_work_tree = std::env::var_os("GIT_WORK_TREE");
        unsafe {
            std::env::set_var("GIT_DIR", &bare);
            std::env::set_var("GIT_WORK_TREE", &home);
        }
        prepare_tracker(&tracker, true, false).unwrap();
        unsafe {
            match old_git_dir {
                Some(value) => std::env::set_var("GIT_DIR", value),
                None => std::env::remove_var("GIT_DIR"),
            }
            match old_git_work_tree {
                Some(value) => std::env::set_var("GIT_WORK_TREE", value),
                None => std::env::remove_var("GIT_WORK_TREE"),
            }
        }

        let top = std::process::Command::new("git")
            .arg("-C")
            .arg(&tracker)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .unwrap();
        assert!(top.status.success(), "tracker rev-parse failed");
        assert_eq!(
            String::from_utf8_lossy(&top.stdout).trim(),
            tracker.display().to_string()
        );
        assert_eq!(git_bare(&["config", "--get", "core.bare"]), before_bare);
        assert_eq!(
            git_bare(&["config", "--get", "core.worktree"]),
            before_worktree
        );
        assert_eq!(
            git_bare(&["config", "--get", "merge.ljos-atoms.driver"]),
            before_driver
        );
    }

    #[test]
    fn the_atoms_driver_stops_before_the_pack_when_the_tracker_has_no_scope() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let out = crate::git_at(root)
            .args(["init", "-q", "-b", "main"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let host = crate::sync::host();
        let rel = format!(".ljos/atoms/{host}.digest");
        std::fs::create_dir_all(root.join(".ljos/atoms")).unwrap();
        let current = root.join(".ljos/atoms").join(format!("{host}.digest"));
        std::fs::write(&current, "local\n").unwrap();
        let err = merge_atoms_driver(&current, &rel).unwrap_err();
        assert!(err.contains("names no scope"), "{err}");
        assert_eq!(std::fs::read_to_string(&current).unwrap(), "local\n");
        let other = merge_atoms_driver(&current, ".ljos/atoms/otherhost.digest").unwrap_err();
        assert!(other.contains("another machine"), "{other}");
    }

    #[test]
    fn three_clones_race_and_atoms_are_reexported() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let host = crate::sync::host();
        let digest = format!(".ljos/atoms/{host}.digest");
        let age = format!(".ljos/atoms/{host}.jsonl.age");
        let script = dir.path().join("atoms-driver");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 're-exported %s\\n' \"$2\" > \"$1\"\nexit 0\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let driver = format!("{} %A %P", script.display());
        let previous = std::env::var_os("LJOS_ATOMS_DRIVER");
        unsafe { std::env::set_var("LJOS_ATOMS_DRIVER", &driver) };
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
        let clones: Vec<PathBuf> = ["a", "b", "c"]
            .into_iter()
            .map(|name| dir.path().join(name))
            .collect();
        run(
            dir.path(),
            &[
                "clone",
                "-q",
                origin.to_str().unwrap(),
                clones[0].to_str().unwrap(),
            ],
        );
        configure(&clones[0]);
        let preamble = "#+TITLE: probe\n\n";
        let seed = heading("seed", "Seed");
        std::fs::write(clones[0].join("issues.org"), format!("{preamble}{seed}")).unwrap();
        std::fs::write(
            clones[0].join(".gitattributes"),
            "*.org merge=union\n.ljos/atoms/* merge=ljos-atoms\n",
        )
        .unwrap();
        std::fs::create_dir_all(clones[0].join(".ljos/atoms")).unwrap();
        std::fs::write(clones[0].join(&digest), "base\n").unwrap();
        std::fs::write(clones[0].join(&age), "base\n").unwrap();
        run(
            &clones[0],
            &["add", "issues.org", ".gitattributes", ".ljos/atoms"],
        );
        run(&clones[0], &["commit", "-q", "-m", "seed"]);
        run(&clones[0], &["push", "-q", "-u", "origin", "HEAD"]);
        for clone in &clones[1..] {
            run(
                dir.path(),
                &[
                    "clone",
                    "-q",
                    origin.to_str().unwrap(),
                    clone.to_str().unwrap(),
                ],
            );
            configure(clone);
        }
        for (clone, id) in clones.iter().zip(["from-a", "from-b", "from-c"]) {
            std::fs::write(
                clone.join("issues.org"),
                format!("{preamble}{seed}{}", heading(id, id)),
            )
            .unwrap();
            std::fs::write(clone.join(&digest), format!("side-{id}\n")).unwrap();
            std::fs::write(clone.join(&age), format!("side-{id}\n")).unwrap();
            run(clone, &["add", "issues.org", ".ljos/atoms"]);
            run(clone, &["commit", "-q", "-m", id]);
        }
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut joins = Vec::new();
        for clone in clones {
            let barrier = barrier.clone();
            joins.push(std::thread::spawn(move || {
                barrier.wait();
                push_checkout(&clone)
            }));
        }
        let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
        unsafe {
            match previous {
                Some(value) => std::env::set_var("LJOS_ATOMS_DRIVER", value),
                None => std::env::remove_var("LJOS_ATOMS_DRIVER"),
            }
        }
        for (i, result) in results.iter().enumerate() {
            assert!(result.is_ok(), "clone {i}: {result:?}");
        }
        let shown = |spec: &str| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(&origin)
                .args(["show", &format!("HEAD:{spec}")])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let text = shown("issues.org");
        assert!(text.contains(":ID:         seed"), "{text}");
        assert!(text.contains(":ID:         from-a"), "{text}");
        assert!(text.contains(":ID:         from-b"), "{text}");
        assert!(text.contains(":ID:         from-c"), "{text}");
        assert!(!text.contains("<<<<<<<"), "{text}");
        for spec in [&digest, &age] {
            let body = shown(spec);
            assert!(body.contains("re-exported"), "{spec}: {body}");
            assert!(body.contains(spec), "{spec}: {body}");
            assert!(!body.contains("side-"), "{spec}: {body}");
            assert!(!body.contains("<<<<<<<"), "{spec}: {body}");
        }

        fn configure(dir: &Path) {
            for (key, value) in [
                ("user.email", "seat@example.invalid"),
                ("user.name", "seat"),
                ("core.hooksPath", "/dev/null"),
                ("commit.gpgsign", "false"),
            ] {
                let out = std::process::Command::new("git")
                    .arg("-C")
                    .arg(dir)
                    .args(["config", key, value])
                    .output()
                    .unwrap();
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }

    #[test]
    fn a_rebase_conflict_merges_and_reexports_atoms() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let origin = dir.path().join("origin.git");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let count = dir.path().join("driver-count");
        let host = crate::sync::host();
        let digest = format!(".ljos/atoms/{host}.digest");
        let script = dir.path().join("atoms-driver");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nf='{}'\nn=0\nif [ -f \"$f\" ]; then n=$(cat \"$f\"); fi\nn=$((n + 1))\nprintf '%s\\n' \"$n\" > \"$f\"\nif [ \"$n\" -eq 1 ]; then exit 1; fi\nprintf 're-exported %s\\n' \"$2\" > \"$1\"\nexit 0\n",
                count.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let driver = format!("{} %A %P", script.display());
        let previous = std::env::var_os("LJOS_ATOMS_DRIVER");
        unsafe { std::env::set_var("LJOS_ATOMS_DRIVER", &driver) };
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
        std::fs::write(
            a.join(".gitattributes"),
            "*.org merge=union\n.ljos/atoms/* merge=ljos-atoms\n",
        )
        .unwrap();
        std::fs::create_dir_all(a.join(".ljos/atoms")).unwrap();
        std::fs::write(a.join(&digest), "base\n").unwrap();
        std::fs::write(a.join("issues.org"), heading("seed", "Seed")).unwrap();
        run(&a, &["add", ".gitattributes", "issues.org", ".ljos/atoms"]);
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
        std::fs::write(a.join(&digest), "side-a\n").unwrap();
        run(&a, &["add", ".ljos/atoms"]);
        run(&a, &["commit", "-q", "-m", "from a"]);
        run(&a, &["push", "-q"]);
        std::fs::write(b.join(&digest), "side-b\n").unwrap();
        run(&b, &["add", ".ljos/atoms"]);
        run(&b, &["commit", "-q", "-m", "from b"]);
        let pushed = push_checkout(&b);
        unsafe {
            match previous {
                Some(value) => std::env::set_var("LJOS_ATOMS_DRIVER", value),
                None => std::env::remove_var("LJOS_ATOMS_DRIVER"),
            }
        }
        assert!(pushed.is_ok(), "{pushed:?}");
        let body = std::process::Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["show", &format!("HEAD:{digest}")])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&body.stdout);
        assert!(text.contains("re-exported"), "{text}");
        assert!(text.contains(&digest), "{text}");
        assert!(!text.contains("side-"), "{text}");
        let parents = std::process::Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["rev-list", "--parents", "-n", "1", "HEAD"])
            .output()
            .unwrap();
        let line = String::from_utf8_lossy(&parents.stdout);
        let parent_count = line.split_whitespace().count();
        assert!(
            parent_count >= 3,
            "merge commit should have two parents: {line}"
        );
        let calls = std::fs::read_to_string(&count).unwrap_or_default();
        assert!(
            calls.trim().parse::<u32>().unwrap_or(0) >= 2,
            "driver calls: {calls}"
        );
    }

    /// The built `ljos`. `cargo test` compiles the bin next to this harness.
    fn ljos_bin() -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let bin = exe
            .parent()
            .and_then(|deps| deps.parent())
            .map(|debug| debug.join("ljos"))
            .unwrap();
        assert!(bin.is_file(), "ljos binary at {}", bin.display());
        bin
    }

    /// git passes `%A` relative to the work tree. A driver that only accepts
    /// an absolute path errors, and catch-up's merge aborts on the conflict.
    #[test]
    fn a_real_merge_reexports_a_conflicted_log_from_a_relative_temp() {
        let _lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let pack_home = dir.path().join("pack");
        std::fs::create_dir_all(home.join(".config/ljos")).unwrap();
        std::fs::create_dir_all(&pack_home).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let packsetd = std::env::var_os("PATH")
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("packsetd"))
                    .find(|candidate| candidate.is_file())
            })
            .expect("packsetd on PATH; CI builds it from packset develop");
        let mut daemon = std::process::Command::new(packsetd)
            .args(["--port", &port.to_string(), "--home"])
            .arg(&pack_home)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &pack_home)
            .env("PACKSET_HOME", &pack_home)
            .env("PACKSET_AUTH", "off")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("packsetd on PATH");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(daemon.try_wait().unwrap().is_none(), "packsetd exited");
            assert!(
                std::time::Instant::now() < deadline,
                "packsetd did not listen"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let key = home.join(".config/ljos/age.key");
        let made = std::process::Command::new("age-keygen")
            .arg("-o")
            .arg(&key)
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
        let public = std::process::Command::new("age-keygen")
            .arg("-y")
            .arg(&key)
            .output()
            .unwrap();
        assert!(public.status.success());
        let public = String::from_utf8_lossy(&public.stdout).trim().to_string();
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
            std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
            std::env::set_var("PACKSET_URL", format!("http://127.0.0.1:{port}"));
            std::env::set_var("PACKSET_AUTH", "off");
            std::env::set_var("PACKSET_HOME", &pack_home);
            std::env::set_var("PACKSET_WORKSPACE", "merge-relative");
            std::env::remove_var("LJOS_WRITE_SCOPE");
        }
        let client = crate::pack().unwrap();
        crate::admit::post_kept(
            &client,
            &serde_json::json!({
                "kind": "lesson",
                "level": "explicit",
                "text": "Re-exported by the atoms driver.",
                "workspace": client.workspace(),
                "entities": ["scope:grokbot"],
            }),
        )
        .unwrap();
        let record = dir.path().join("percent-a");
        let wrapper = dir.path().join("atoms-driver");
        let ljos = ljos_bin();
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$1\" > {}\nexec {} merge-atoms \"$1\" \"$2\"\n",
                crate::tools::sh_quote(&record.display().to_string()),
                crate::tools::sh_quote(&ljos.display().to_string()),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let driver = format!("{} %A %P", wrapper.display());
        let previous = std::env::var_os("LJOS_ATOMS_DRIVER");
        unsafe { std::env::set_var("LJOS_ATOMS_DRIVER", &driver) };
        let origin = dir.path().join("origin.git");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let host = crate::sync::host();
        let rel = format!(".ljos/atoms/{host}.jsonl.age");
        let run = |cwd: &std::path::Path, args: &[&str]| {
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
        std::fs::create_dir_all(a.join(".ljos/atoms")).unwrap();
        std::fs::write(
            a.join(".ljos/sync.toml"),
            format!("name = \"grokbot\"\nrecipients = [\"{public}\"]\n"),
        )
        .unwrap();
        std::fs::write(a.join(&rel), "base\n").unwrap();
        prepare_tracker(&a, false, false).unwrap();
        run(&a, &["add", ".gitattributes", ".ljos"]);
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
        prepare_tracker(&b, false, false).unwrap();
        std::fs::write(a.join(&rel), "side-a\n").unwrap();
        run(&a, &["add", ".ljos/atoms"]);
        run(&a, &["commit", "-q", "-m", "from a"]);
        run(&a, &["push", "-q"]);
        std::fs::write(b.join(&rel), "side-b\n").unwrap();
        run(&b, &["add", ".ljos/atoms"]);
        run(&b, &["commit", "-q", "-m", "from b"]);
        run(&b, &["fetch", "-q", "origin"]);
        let merged = std::process::Command::new("git")
            .arg("-C")
            .arg(&b)
            .args(["merge", "--no-edit", "origin/main"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        unsafe {
            match previous {
                Some(value) => std::env::set_var("LJOS_ATOMS_DRIVER", value),
                None => std::env::remove_var("LJOS_ATOMS_DRIVER"),
            }
        }
        let _ = daemon.kill();
        let _ = daemon.wait();
        assert!(
            merged.status.success(),
            "merge: {}\n{}",
            String::from_utf8_lossy(&merged.stdout),
            String::from_utf8_lossy(&merged.stderr)
        );
        let passed = std::fs::read_to_string(&record).unwrap();
        let passed = passed.trim();
        assert!(
            !passed.starts_with('/'),
            "git passed an absolute %A, so this did not exercise the relative temp: {passed}"
        );
        assert!(
            passed.contains("merge_file"),
            "git %A was not its merge temp: {passed}"
        );
        let opened = std::process::Command::new("age")
            .args(["-d", "-i"])
            .arg(&key)
            .arg(b.join(&rel))
            .output()
            .unwrap();
        assert!(
            opened.status.success(),
            "age -d: {}",
            String::from_utf8_lossy(&opened.stderr)
        );
        let plain = String::from_utf8_lossy(&opened.stdout);
        assert!(
            plain.contains("Re-exported by the atoms driver."),
            "{plain}"
        );
        assert!(
            !plain.contains("side-a") && !plain.contains("side-b"),
            "{plain}"
        );
        assert!(!plain.contains("<<<<<<<"), "{plain}");
    }
}
