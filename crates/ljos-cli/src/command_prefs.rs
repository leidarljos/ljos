//! Preferences at the moment the agent acts.
//!
//! "Use pnpm instead of npm" came back on a prompt that shared its words,
//! and `npm install lodash` still ran with nothing said. Two things close
//! that: a preference that forbids a command it can name proposes an ask
//! rule over that command ([`rule_candidates`]), and the tool-call hook
//! hands over the preferences that name the command about to run
//! ([`naming_command`]). Both read words only, so neither costs the hook
//! more than the pack listing it already makes for the rules.

use super::Rule;

/// Openings after which a preference names what it forbids. "Ask before"
/// asks, the rest forbid; the rule is an ask either way, so the person
/// who is stopped decides.
const CUES: &[&str] = &[
    "instead of",
    "rather than",
    "ask before",
    "check before",
    "never",
    "don't",
    "do not",
    "avoid",
    "stop using",
    "not ",
];

/// A word the forbidden part names, and the ask rule it gives.
const TABLE: &[(&[&str], &str)] = &[
    (&["npm"], r"re:npm\s+(install|i|add|ci)\b"),
    (&["yarn"], r"re:yarn(\s+(add|install)\b|\s*$)"),
    (&["pip", "pip3"], r"re:(python3?\s+-m\s+)?pip3?\s+install\b"),
    (
        &["force-push", "force"],
        r"re:git\s+push\b.*(--force\b|-f\b)",
    ),
    (&["push"], r"re:git\s+push\b.*\b(main|master)\b"),
    (&[".env"], r"re:.*>>?\s*\S*\.env\b"),
    (
        &["migration", "migrations", "migrate"],
        r"re:(psql\b.*-f\s*\S*migrat|rails\s+db:migrate|bin/rails\s+db:migrate|alembic\s+upgrade|flyway\s+migrate|prisma\s+migrate\s+deploy|sqlx\s+migrate\s+run|diesel\s+migration\s+run|knex\s+migrate|goose\b.*\bup\b)",
    ),
    (
        &["terraform", "tofu"],
        r"re:(terraform|tofu)\b.*\b(destroy|apply\b.*-destroy)\b",
    ),
    (&["kubectl"], r"re:kubectl\b.*\bdelete\b"),
    (
        &["docker"],
        r"re:docker\s+(system\s+prune|volume\s+(rm|prune)|rm|rmi)\b",
    ),
];

/// Words that tie a migration preference to a shared database.
const REMOTE_WORDS: &[&str] = &["staging", "stage", "prod", "production", "live"];

/// A migration run that names a shared database, in either order.
const MIGRATION_REMOTE: &str = r"re:.*\b(staging|stage|prod|production|live)\b.*\b(migrat\w*|upgrade)\b|.*\b(migrat\w*|upgrade)\b.*\b(staging|stage|prod|production|live)\b";

/// Words that make a migration preference about running one.
const RUN_WORDS: &[&str] = &[
    "run",
    "running",
    "apply",
    "applying",
    "against",
    "migrate",
    "migrating",
];

/// The forbidden part of a preference: what follows its first cue, up to
/// the end of that clause. `None` when it has no cue.
fn forbidden_part(text: &str) -> Option<String> {
    let lower = format!(" {} ", text.to_lowercase());
    let (at, cue) = CUES
        .iter()
        .filter_map(|c| lower.find(&format!(" {c}")).map(|i| (i, *c)))
        .min_by_key(|(i, _)| *i)?;
    let rest = &lower[at + 1 + cue.len()..];
    let end = rest.find([';', '.', ':']).map_or(rest.len(), |i| {
        // A dot inside a word, as in `.env`, does not end the clause.
        let next = rest[i + 1..].chars().next();
        if rest.as_bytes()[i] == b'.' && next.is_some_and(char::is_alphanumeric) {
            rest[i + 1..]
                .find([';', ':'])
                .map_or(rest.len(), |j| i + 1 + j)
        } else {
            i
        }
    });
    Some(rest[..end].to_string())
}

/// Words of a clause, kept with their dots so `.env` stays one word.
fn words(clause: &str) -> Vec<String> {
    clause
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')' | '"' | '`' | '\''))
        .map(|w| w.trim_end_matches(['.', '!', '?']).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// The ask rules a preference proposes: one per command its forbidden
/// part names in [`TABLE`], each with the preference as its reason.
/// "Use pnpm instead of npm" gives one over `npm install`; "Use tabs in
/// Go files" names nothing it forbids and gives none.
#[must_use]
pub fn rule_candidates(text: &str) -> Vec<Rule> {
    let Some(part) = forbidden_part(text) else {
        return Vec::new();
    };
    let named = words(&part);
    let mut rules: Vec<Rule> = Vec::new();
    for (keys, pattern) in TABLE {
        if keys.iter().any(|k| named.iter().any(|w| w == k))
            && !rules.iter().any(|r| r.pattern == *pattern)
        {
            // A migration rule asks before one runs, so the preference
            // must be about running one: "never edit a migration that has
            // been applied" is about the file.
            if keys.contains(&"migration") && !named.iter().any(|w| RUN_WORDS.contains(&w.as_str()))
            {
                continue;
            }
            // "force push" is one rule, not a force rule and a main one.
            if *pattern == TABLE[4].1 && rules.iter().any(|r| r.pattern == TABLE[3].1) {
                continue;
            }
            // "Against staging or production" asks there and not on a
            // migration run against a dev database.
            let pattern = if keys.contains(&"migration")
                && named.iter().any(|w| REMOTE_WORDS.contains(&w.as_str()))
            {
                MIGRATION_REMOTE
            } else {
                *pattern
            };
            if rules.iter().any(|r| r.pattern == pattern) {
                continue;
            }
            rules.push(Rule {
                pattern: pattern.to_string(),
                verdict: "ask".into(),
                reason: text.trim().to_string(),
            });
        }
    }
    rules
}

/// Words too common to tie a preference to a command.
const STOP: &[&str] = &[
    "a", "an", "and", "any", "are", "as", "at", "be", "before", "by", "every", "for", "from", "in",
    "into", "is", "it", "its", "of", "on", "or", "our", "the", "them", "this", "to", "use",
    "using", "run", "running", "with", "when", "we", "you", "your", "always", "never", "instead",
    "prefer", "keep", "make", "do", "not", "plain", "file", "files", "repo", "here", "all", "each",
    "new", "via", "than", "rather", "only", "first", "then",
];

/// Stem a word just enough that "migrations" meets "migration".
fn stem(word: &str) -> &str {
    word.strip_suffix('s')
        .filter(|s| s.len() >= 3)
        .unwrap_or(word)
}

/// What a command line says: per segment, the program, its subcommand,
/// the files it writes to, and every word piece of its arguments.
struct CommandWords {
    heads: Vec<String>,
    pieces: Vec<String>,
}

fn command_words(line: &str) -> CommandWords {
    let mut heads = Vec::new();
    let mut pieces = Vec::new();
    for segment in line.split(['|', ';', '&', '\n']) {
        let mut toks = segment.split_whitespace().peekable();
        // `FOO=1 sudo env npm i` runs npm.
        while let Some(t) = toks.peek() {
            let assignment = t.split_once('=').is_some_and(|(k, _)| {
                !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            });
            if assignment || matches!(*t, "sudo" | "env" | "command" | "exec" | "time" | "nice") {
                toks.next();
            } else {
                break;
            }
        }
        let Some(program) = toks.next() else { continue };
        let program = program.rsplit('/').next().unwrap_or(program).to_lowercase();
        heads.push(program.clone());
        let rest: Vec<&str> = toks.collect();
        if let Some(sub) = rest.iter().find(|w| !w.starts_with('-')) {
            if matches!(
                program.as_str(),
                "git" | "cargo" | "go" | "docker" | "kubectl" | "npm"
            ) {
                heads.push(sub.to_lowercase());
            }
        }
        for (i, w) in rest.iter().enumerate() {
            let target = if let Some(t) = w.strip_prefix(">>").or_else(|| w.strip_prefix('>')) {
                if t.is_empty() {
                    rest.get(i + 1).copied()
                } else {
                    Some(t)
                }
            } else {
                None
            };
            if let Some(t) = target {
                let base = t.rsplit('/').next().unwrap_or(t).to_lowercase();
                heads.push(base);
            }
            for p in w.split(|c: char| !c.is_alphanumeric()) {
                if p.len() >= 3 && !p.chars().all(|c| c.is_ascii_digit()) {
                    pieces.push(p.to_lowercase());
                }
            }
        }
    }
    CommandWords { heads, pieces }
}

/// Whether a preference bears on a command line. It does when the words
/// it forbids (or, with no cue, all its words) name the command's
/// program, its subcommand or a file it writes to, or share two content
/// words with the command's arguments. "Use pnpm instead of npm" bears on
/// `npm install lodash` and not on `pnpm add lodash`; "Ask before running
/// any database migration against staging" bears on
/// `psql -h staging-db -f migrations/007.sql`.
#[must_use]
pub fn bears_on(preference: &str, line: &str) -> bool {
    let clause = forbidden_part(preference).unwrap_or_else(|| preference.to_lowercase());
    let named: Vec<String> = words(&clause)
        .into_iter()
        .filter(|w| !STOP.contains(&w.as_str()) && w.len() >= 2)
        .collect();
    if named.is_empty() {
        return false;
    }
    let cmd = command_words(line);
    if cmd.heads.iter().any(|h| named.iter().any(|w| w == h)) {
        return true;
    }
    let mut shared: Vec<&str> = named
        .iter()
        .map(|w| stem(w))
        .filter(|w| cmd.pieces.iter().any(|p| stem(p) == *w))
        .collect();
    shared.sort_unstable();
    shared.dedup();
    shared.len() >= 2
}

/// The preferences, as (id, text), that bear on `line`, at most `cap`.
#[must_use]
pub fn naming_command<'a>(
    preferences: &'a [(String, String)],
    line: &str,
    cap: usize,
) -> Vec<&'a (String, String)> {
    if line.trim().is_empty() {
        return Vec::new();
    }
    preferences
        .iter()
        .filter(|(_, text)| bears_on(text, line))
        .take(cap)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rule_matches;

    const PREFS: &[&str] = &[
        "Use pnpm instead of npm for every JavaScript package install in this repo",
        "Run cargo clippy with -D warnings before every commit",
        "Never push directly to main; open a pull request from a feature branch",
        "Write commit subjects in the imperative mood with a conventional prefix such as fix: or feat:",
        "Use uv to run Python tools instead of pip or plain python",
        "Keep secrets in 1Password and never write them into .env files",
        "Format SQL migrations with sqlfluff and never edit a migration that has been applied",
        "Use tabs for indentation in Go files and gofmt them",
        "Ask before running any database migration against staging or production",
        "Prefer table-driven tests in Go and parametrize in pytest",
    ];

    /// Each preference's candidates, as the patterns they match on.
    fn asks(text: &str, line: &str) -> bool {
        rule_candidates(text)
            .iter()
            .any(|r| rule_matches(&r.pattern, line))
    }

    #[test]
    fn a_forbidding_preference_proposes_an_ask_over_its_command() {
        assert!(asks(PREFS[0], "npm install lodash"));
        assert!(asks(PREFS[0], "npm i -D vitest"));
        assert!(!asks(PREFS[0], "pnpm add lodash"));
        assert!(!asks(PREFS[0], "npm test"));
        assert!(asks(PREFS[2], "git push origin main"));
        assert!(!asks(PREFS[2], "git push origin feature/x"));
        assert!(asks(PREFS[4], "pip install black"));
        assert!(asks(PREFS[4], "python3 -m pip install -r requirements.txt"));
        assert!(!asks(PREFS[4], "uv pip install black"));
        assert!(asks(PREFS[5], "echo STRIPE_KEY=sk_live_x >> .env"));
        assert!(!asks(PREFS[5], "cat .env.example"));
        assert!(asks(PREFS[8], "psql -h staging-db -f migrations/007.sql"));
        assert!(asks(PREFS[8], "RAILS_ENV=production bin/rails db:migrate"));
        assert!(asks(PREFS[8], "alembic -x env=prod upgrade head"));
        assert!(!asks(PREFS[8], "alembic upgrade head"));
        assert!(!asks(PREFS[8], "psql -f migrations/001_init.sql app_dev"));
        assert!(asks(
            "Ask before running a database migration",
            "bin/rails db:migrate"
        ));
        assert!(!asks(PREFS[8], "psql -h staging-db -c 'select 1'"));
        assert!(asks(
            "Never force push to shared branches",
            "git push --force origin dev"
        ));
        assert!(!asks(
            "Never force push to shared branches",
            "git push origin main"
        ));
        assert!(asks(
            "Never run terraform destroy",
            "terraform -chdir=infra destroy"
        ));
        assert!(asks("Don't use yarn here", "yarn add lodash"));
        assert!(asks("Don't use yarn here", "yarn"));
        // Nothing forbidden, or nothing the table knows: no rule.
        for text in [
            PREFS[1],
            PREFS[3],
            PREFS[6],
            PREFS[7],
            PREFS[9],
            "Never use unwrap in library code",
        ] {
            assert!(rule_candidates(text).is_empty(), "{text}");
        }
        let n = PREFS
            .iter()
            .filter(|p| !rule_candidates(p).is_empty())
            .count();
        assert_eq!(n, 5, "five of the ten preferences carry a rule");
        assert!(
            rule_candidates(PREFS[6]).is_empty(),
            "editing a migration is not running one"
        );
        assert!(rule_candidates(PREFS[0])
            .iter()
            .all(|r| r.verdict == "ask" && r.reason == PREFS[0]));
    }

    #[test]
    fn a_preference_bears_on_the_command_it_names() {
        let yes = [
            (0, "npm install lodash"),
            (2, "git push origin main"),
            (1, "git commit -m 'fix: x'"),
            (3, "git commit -m 'fix: x'"),
            (4, "pip install black"),
            (5, "echo STRIPE_KEY=sk_live_x >> .env"),
            (8, "psql -h staging-db -f migrations/007.sql"),
            (9, "pytest -q tests/"),
            (7, "go build ./..."),
        ];
        for (i, line) in yes {
            assert!(bears_on(PREFS[i], line), "{} on {line}", PREFS[i]);
        }
        let no = [
            (0, "pnpm add lodash"),
            (0, "pip install black"),
            (4, "uv run black ."),
            (2, "git status"),
            (5, "cat README.md"),
            (8, "psql -h localhost -c 'select 1'"),
            (6, "ls"),
        ];
        for (i, line) in no {
            assert!(!bears_on(PREFS[i], line), "{} on {line}", PREFS[i]);
        }
        let prefs: Vec<(String, String)> = PREFS
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("id{i}"), (*t).to_string()))
            .collect();
        let hit = naming_command(&prefs, "npm install lodash", 2);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].0, "id0");
        assert!(naming_command(&prefs, "  ", 2).is_empty());
    }
}
