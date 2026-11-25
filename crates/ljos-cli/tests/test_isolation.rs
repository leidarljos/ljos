use std::path::{Path, PathBuf};

fn rust_files(root: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn test_bodies<'a>(source: &'a str, path: &Path) -> Vec<&'a str> {
    let mark = |text: &str, needle: &str| {
        text.match_indices(needle).find_map(|(at, _)| {
            let line = text[..at].rfind('\n').map_or(0, |line| line + 1);
            text[line..at]
                .trim()
                .is_empty()
                .then_some((at, needle.len()))
        })
    };
    let mut offset = 0;
    let mut starts = Vec::new();
    loop {
        let rest = &source[offset..];
        let plain = mark(rest, "#[test]");
        let async_test = mark(rest, "#[tokio::test");
        let Some((mark, len)) = plain
            .into_iter()
            .chain(async_test)
            .min_by_key(|(at, _)| *at)
        else {
            break;
        };
        let annotation = offset + mark;
        let rest = &rest[mark + len..];
        let function = rest
            .find("fn ")
            .unwrap_or_else(|| panic!("{} has #[test] without a function", path.display()));
        let function_at = annotation + len + function;
        let indent_start = source[..function_at].rfind('\n').map_or(0, |line| line + 1);
        let indent = source[indent_start..function_at]
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .collect::<String>();
        let rest = &rest[function..];
        let open = rest
            .find('{')
            .unwrap_or_else(|| panic!("{} has a test without a body", path.display()));
        let body = annotation + len + function + open + 1;
        starts.push((function_at, body, indent));
        offset = body;
    }
    starts
        .iter()
        .map(|(function, body, indent)| {
            let close = format!("{indent}}}");
            let mut end = source.len();
            let mut consumed = 0;
            for line in source[*body..].split_inclusive('\n') {
                if line.trim_end_matches(['\r', '\n']) == close {
                    end = *body + consumed;
                    break;
                }
                consumed += line.len();
            }
            &source[*function..end]
        })
        .collect()
}

fn has_nearby(body: &str, needle: &str) -> bool {
    body.lines().take(8).any(|line| line.contains(needle))
}

fn has_process_guard(body: &str) -> bool {
    has_nearby(body, "test_env_read_lock()")
        || has_nearby(body, "test_env_lock()")
        || has_nearby(body, "env_guard()")
}

fn has_process_writer(body: &str) -> bool {
    [
        "test_env_lock()",
        "env_guard()",
        "env_lock()",
        "test_home()",
        "review_env()",
        "Shared::new(",
        "isolated()",
        "begin(",
        "hold_env(",
        "hud_env(",
        "pack_aside(",
    ]
    .iter()
    .any(|guard| body.contains(guard))
}

fn mutates_process_state(source: &str) -> bool {
    let compact: String = source.chars().filter(|ch| !ch.is_whitespace()).collect();
    ["set_var(", "remove_var(", "set_current_dir("]
        .iter()
        .any(|call| compact.contains(call))
}

#[test]
fn process_global_test_state_is_guarded() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest.join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    for path in files {
        if path == src.join("main.rs") || path.starts_with(src.join("bin")) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for body in test_bodies(&source, &path) {
            assert!(
                has_process_guard(body),
                "{} has a unit test without a process-state guard: {:?}",
                path.display(),
                body.lines().take(3).collect::<Vec<_>>()
            );
            if mutates_process_state(body) {
                assert!(
                    has_process_writer(body),
                    "{} mutates process state in a unit test without the writer: {:?}",
                    path.display(),
                    body.lines().take(3).collect::<Vec<_>>()
                );
            }
        }
    }

    let main = src.join("main.rs");
    let source = std::fs::read_to_string(&main).unwrap();
    for body in test_bodies(&source, &main) {
        assert!(
            has_process_guard(body),
            "{} has a unit test without a process-state guard",
            main.display()
        );
        if mutates_process_state(body) {
            assert!(
                has_process_writer(body),
                "{} mutates process state in a unit test without the writer",
                main.display()
            );
        }
    }
    let mut bins = Vec::new();
    rust_files(&src.join("bin"), &mut bins);
    for path in bins {
        let source = std::fs::read_to_string(&path).unwrap();
        for body in test_bodies(&source, &path) {
            assert!(
                has_nearby(body, "crate::process_lock()"),
                "{} has a binary unit test without the process-state writer",
                path.display()
            );
        }
    }

    let mut integration = Vec::new();
    rust_files(&manifest.join("tests"), &mut integration);
    let hud = manifest.parent().unwrap().join("ljos-hud/src");
    rust_files(&hud, &mut integration);
    for path in integration {
        if path == manifest.join("tests/test_isolation.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        if path.starts_with(&hud) {
            for body in test_bodies(&source, &path) {
                assert!(
                    has_nearby(body, "ljos_test_boot::env_read_lock()"),
                    "{} has a HUD unit test without the process-state reader",
                    path.display()
                );
            }
        }
        if !mutates_process_state(&source) {
            continue;
        }
        for body in test_bodies(&source, &path) {
            assert!(
                has_nearby(body, "ljos_test_boot::env_lock()"),
                "{} mutates process state but has a test without the shared writer",
                path.display()
            );
        }
    }
}

#[test]
fn process_mutation_scan_catches_imports_and_aliases() {
    assert!(mutates_process_state(
        "use std::env; env::set_var(\"NO_COLOR\", \"1\");"
    ));
    assert!(mutates_process_state(
        "use std::env::remove_var; remove_var (\"NO_COLOR\");"
    ));
    assert!(mutates_process_state(
        "use std::env as process_env; process_env::set_current_dir(\"/\");"
    ));
}

#[test]
fn test_body_scan_stops_at_the_next_test() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("src/digest.rs");
    let source = std::fs::read_to_string(&path).unwrap();
    let first = test_bodies(&source, &path)[0];
    assert!(!mutates_process_state(first), "{first}");
}
