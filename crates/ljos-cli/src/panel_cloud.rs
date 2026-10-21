//! One prompt file per persona for a cloud panel.
//!
//! The file names the seat environment, the slot and the push rules. The
//! brief stays a command the seat runs after `ljos cloud up`, so pack text
//! is not copied into the file. The signed slot note is written when the
//! run id exists; this command only records the line that note will carry.

use std::path::Path;

use anyhow::{Context, Result};

/// Model families a cloud panel rotates, in persona name order. Grok is
/// first. A two-seat panel therefore names two families.
pub const FAMILIES: &[&str] = &["grok", "opus", "codex"];

/// The family for persona `index` after the roster is sorted by name.
#[must_use]
pub fn family(index: usize) -> &'static str {
    FAMILIES[index % FAMILIES.len()]
}

/// The prompt one cloud seat starts from.
///
/// `family` is the `model` on the slot line. The seat value is the persona's
/// slug; `ljos cloud up` appends the run id.
#[must_use]
pub fn prompt(persona: &str, issue: &str, family: &str) -> String {
    let seat = crate::seat_slug(persona);
    format!(
        "\
You are {persona} on {issue}. This file is the only prompt this run starts \
from. Other runs' transcripts are not here. Read the brief with \
`ljos brief {persona} {issue}` and follow it. Do not read `vissue vote \
{issue}`, `vissue show {issue}` or `ljos consensus {issue}` until the \
ballot is cast.

== seat
LJOS_SEAT={seat}
VISSUE_AGENT={seat}
LJOS_TRACKER_GIT=push

Set these, then run `ljos cloud up`. It appends this run's id, at most 16 \
characters, to LJOS_SEAT and writes that seat's key under \
`~/.local/share/ljos/seats/`. The key stays on this machine. Leave \
LJOS_SEAT and VISSUE_AGENT as `cloud up` wrote them. The tracker URL \
comes from the launcher \
as LJOS_TRACKER_REMOTE and is not written here. When that variable is set, \
`cloud up` turns push mode on.

Cast the ballot as {persona} (`ljos vote {issue} --for OPTION --as \
{persona}`). The seat that signs it is the one `cloud up` wrote.

== slot
slot: {seat} model={family}

The launcher signs `slot: SEAT model={family} run=RUN` under its own pinned \
key when it starts this run. SEAT is {seat} with the run id appended. RUN \
is that id. The ballot counts only under that slot. Stay on this seat and \
on model {family}.

== push
Push the tracker through ljos with LJOS_TRACKER_GIT=push. Do not push a \
ref under cursor/. Do not run `gh pr create` or `gh pr merge`. Do not \
force-push.
"
    )
}

/// Write one prompt file per seated persona under `out`.
///
/// Files are named `<persona>.md`. Families rotate in persona name order.
///
/// # Errors
///
/// As [`crate::panel_roster`], or a prompt file cannot be written.
pub fn panel_cloud(issue: &str, out: &Path) -> Result<String> {
    let (all, mut personas) = crate::panel_roster(issue)?;
    personas.sort_by(|a, b| a.name.cmp(&b.name));
    std::fs::create_dir_all(out).with_context(|| format!("panel --cloud: {}", out.display()))?;
    let mut lines = vec![format!(
        "{} of {all} personas speak to {issue}; cloud prompts in {}; start one cloud seat per \
         file. The launcher signs each slot when the run starts, then:",
        personas.len(),
        out.display()
    )];
    for (index, persona) in personas.iter().enumerate() {
        let family = family(index);
        let path = out.join(format!("{}.md", persona.name));
        std::fs::write(&path, prompt(&persona.name, issue, family))
            .with_context(|| format!("panel --cloud: {}", path.display()))?;
        lines.push(format!(
            "  {}  seat {}  model {family}",
            path.display(),
            crate::seat_slug(&persona.name)
        ));
    }
    lines.push(format!("ljos consensus {issue}"));
    Ok(lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_rotate_and_a_pair_differs() {
        assert_eq!(family(0), "grok");
        assert_eq!(family(1), "opus");
        assert_eq!(family(2), "codex");
        assert_eq!(family(3), "grok");
        assert_ne!(family(0), family(1));
    }

    #[test]
    fn a_prompt_names_the_seat_the_slot_and_the_push_rules() {
        let text = prompt("Ada Rider", "probe-l0g0", "opus");
        assert!(text.contains("You are Ada Rider on probe-l0g0."), "{text}");
        assert!(text.contains("ljos brief Ada Rider probe-l0g0"), "{text}");
        assert!(text.contains("LJOS_SEAT=ada-rider\n"), "{text}");
        assert!(text.contains("VISSUE_AGENT=ada-rider\n"), "{text}");
        assert!(text.contains("LJOS_TRACKER_GIT=push\n"), "{text}");
        assert!(text.contains("slot: ada-rider model=opus\n"), "{text}");
        assert!(text.contains("slot: SEAT model=opus run=RUN"), "{text}");
        assert!(
            text.contains("ljos vote probe-l0g0 --for OPTION --as Ada Rider"),
            "{text}"
        );
        assert!(text.contains("vissue vote probe-l0g0"), "{text}");
        assert!(text.contains("vissue show probe-l0g0"), "{text}");
        assert!(text.contains("ljos consensus probe-l0g0"), "{text}");
        assert!(text.contains("ref under cursor/"), "{text}");
        assert!(text.contains("gh pr create"), "{text}");
        assert!(text.contains("gh pr merge"), "{text}");
        assert!(text.contains("force-push"), "{text}");
        assert!(text.contains("LJOS_TRACKER_REMOTE"), "{text}");
        assert!(!text.contains("secret-remote"), "{text}");
        assert!(!text.contains("example.invalid"), "{text}");
        assert!(!text.contains("What this seat knows"), "{text}");
        assert!(!text.contains("What you remembered"), "{text}");
    }
}
