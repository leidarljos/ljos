use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::path::{Path, PathBuf};
use syn::visit::Visit;

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

/// The guard opens a test. One `use` or scratch value may sit above it.
const GUARD_WITHIN: usize = 2;

/// One test function, read from the syntax tree. Comments and string
/// literals are not tokens of it, so neither can stand in for a guard.
struct TestFn {
    name: String,
    line: usize,
    /// Each top-level `let NAME = path(..);` in the body: the statement's
    /// index, the name and the path called. A guard counts only when that
    /// name is still bound at the end of the body.
    bindings: Vec<(usize, String, String)>,
    /// Names ended by `drop(name)` or `let _ = name` before the body ends.
    ended: Vec<String>,
    /// Every function, method and macro-argument call in the body.
    calls: Vec<String>,
}

impl TestFn {
    fn guarded(&self, guards: &[&str]) -> bool {
        self.bindings.iter().any(|(at, name, path)| {
            *at < GUARD_WITHIN
                && names_any(path, guards)
                && !self.ended.iter().any(|ended| ended == name)
        })
    }

    fn calls_any(&self, names: &[&str]) -> bool {
        self.calls.iter().any(|path| names_any(path, names))
    }
}

/// `path` is one of `names`, whole or as its last segments: `env_guard`
/// matches `crate::tests::env_guard`, not `my_env_guard`.
fn names_any(path: &str, names: &[&str]) -> bool {
    names.iter().any(|name| {
        path == *name
            || path
                .strip_suffix(name)
                .is_some_and(|head| head.ends_with("::"))
    })
}

fn path_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn is_test_attr(attr: &syn::Attribute) -> bool {
    let path = path_name(attr.path());
    path == "test" || path == "tokio::test"
}

/// `#` and `[test]` or `[tokio::test ..]` at `at`, read from tokens.
fn test_attr_at(trees: &[TokenTree], at: usize) -> bool {
    let (Some(TokenTree::Punct(hash)), Some(TokenTree::Group(group))) =
        (trees.get(at), trees.get(at + 1))
    else {
        return false;
    };
    if hash.as_char() != '#' || group.delimiter() != Delimiter::Bracket {
        return false;
    }
    let words: Vec<String> = group
        .stream()
        .into_iter()
        .take(4)
        .map(|tree| tree.to_string())
        .collect();
    words == ["test"] || words.starts_with(&["tokio".into(), ":".into(), ":".into(), "test".into()])
}

fn count_test_attrs(tokens: TokenStream) -> usize {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    (0..trees.len())
        .map(|at| {
            let nested = match &trees[at] {
                TokenTree::Group(group) => count_test_attrs(group.stream()),
                _ => 0,
            };
            nested + usize::from(test_attr_at(&trees, at))
        })
        .sum()
}

/// Calls in a token stream: a path, or a method name after `.`, followed by
/// a parenthesised group. Macro arguments are tokens to syn, so this is how
/// a call inside `assert!(..)` or a `macro_rules!` body is seen.
fn token_calls(tokens: TokenStream, out: &mut Vec<String>) {
    let mut path = String::new();
    let mut joined = false;
    let mut colons = 0;
    let mut after_fn = false;
    for tree in tokens {
        match tree {
            TokenTree::Ident(ident) => {
                let word = ident.to_string();
                if joined {
                    path.push_str("::");
                    path.push_str(&word);
                } else if after_fn {
                    path.clear();
                } else {
                    path = word.clone();
                }
                after_fn = word == "fn";
                joined = false;
                colons = 0;
            }
            TokenTree::Punct(punct) if punct.as_char() == ':' => {
                colons += 1;
                if colons == 2 {
                    joined = !path.is_empty();
                    colons = 0;
                }
            }
            TokenTree::Group(group) => {
                if group.delimiter() == Delimiter::Parenthesis && !path.is_empty() && !joined {
                    out.push(path.clone());
                }
                token_calls(group.stream(), out);
                path.clear();
                joined = false;
                after_fn = false;
            }
            _ => {
                path.clear();
                joined = false;
                colons = 0;
                after_fn = false;
            }
        }
    }
}

#[derive(Default)]
struct Calls(Vec<String>);

impl<'ast> Visit<'ast> for Calls {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(func) = &*call.func {
            self.0.push(path_name(&func.path));
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.0.push(call.method.to_string());
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        token_calls(mac.tokens.clone(), &mut self.0);
    }
}

/// `let NAME = path(..)` when `NAME` is a real binding. `let _ = path()`
/// drops the value at once, so it is not a binding. A later `drop(NAME)`
/// or `let _ = NAME` ends it, and a bare `path();` never binds it.
fn guard_binding(stmt: &syn::Stmt) -> Option<(String, String)> {
    let syn::Stmt::Local(local) = stmt else {
        return None;
    };
    let name = binding_name(&local.pat)?;
    let init = local.init.as_ref()?;
    if init.diverge.is_some() {
        return None;
    }
    let syn::Expr::Call(call) = &*init.expr else {
        return None;
    };
    let syn::Expr::Path(func) = &*call.func else {
        return None;
    };
    Some((name, path_name(&func.path)))
}

fn binding_name(pat: &syn::Pat) -> Option<String> {
    let pat = match pat {
        syn::Pat::Type(typed) => &*typed.pat,
        pat => pat,
    };
    let syn::Pat::Ident(ident) = pat else {
        return None;
    };
    let name = ident.ident.to_string();
    if name == "_" {
        None
    } else {
        Some(name)
    }
}

/// One path segment, so `guard` matches and `guard.field` does not.
fn path_leaf(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    Some(path.path.segments[0].ident.to_string())
}

#[derive(Default)]
struct Ended(Vec<String>);

impl<'ast> Visit<'ast> for Ended {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(func) = &*call.func {
            let drop_call = func
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "drop");
            if drop_call && call.args.len() == 1 {
                self.0.extend(call.args.first().and_then(path_leaf));
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if matches!(local.pat, syn::Pat::Wild(_)) {
            if let Some(name) = local
                .init
                .as_ref()
                .filter(|init| init.diverge.is_none())
                .and_then(|init| path_leaf(&init.expr))
            {
                self.0.push(name);
            }
        }
        syn::visit::visit_local(self, local);
    }
}

/// The same binding read from a macro body's tokens, where `$name` and
/// other fragments keep syn from parsing a statement.
fn token_bindings(body: TokenStream) -> Vec<(usize, String, String)> {
    let trees: Vec<TokenTree> = body.into_iter().collect();
    let statements = trees.split(|tree| matches!(tree, TokenTree::Punct(p) if p.as_char() == ';'));
    let mut out = Vec::new();
    for (at, statement) in statements.enumerate() {
        let [TokenTree::Ident(keyword), TokenTree::Ident(name), TokenTree::Punct(eq), rest @ ..] =
            statement
        else {
            continue;
        };
        if keyword != "let" || name == "_" || eq.as_char() != '=' {
            continue;
        }
        let Some((TokenTree::Group(args), path)) = rest.split_last() else {
            continue;
        };
        if args.delimiter() != Delimiter::Parenthesis
            || path.is_empty()
            || !path.iter().all(|tree| match tree {
                TokenTree::Ident(_) => true,
                TokenTree::Punct(p) => p.as_char() == ':',
                _ => false,
            })
        {
            continue;
        }
        let path: String = path.iter().map(ToString::to_string).collect();
        out.push((at, name.to_string(), path));
    }
    out
}

/// `drop(name)` and `let _ = name` in a macro body, where syn cannot parse
/// the statements.
fn token_ended(body: TokenStream) -> Vec<String> {
    let mut ended = Vec::new();
    token_ended_at(&body.into_iter().collect::<Vec<_>>(), &mut ended);
    ended
}

fn token_ended_at(trees: &[TokenTree], ended: &mut Vec<String>) {
    for (at, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            token_ended_at(&group.stream().into_iter().collect::<Vec<_>>(), ended);
        }
        if let Some(name) = dropped_name(trees, at) {
            ended.push(name);
        }
        if let Some(name) = moved_into_wild(trees, at) {
            ended.push(name);
        }
    }
}

fn dropped_name(trees: &[TokenTree], at: usize) -> Option<String> {
    let TokenTree::Ident(word) = &trees[at] else {
        return None;
    };
    if word != "drop" {
        return None;
    }
    let TokenTree::Group(args) = trees.get(at + 1)? else {
        return None;
    };
    if args.delimiter() != Delimiter::Parenthesis {
        return None;
    }
    let inner: Vec<TokenTree> = args.stream().into_iter().collect();
    let [TokenTree::Ident(name)] = inner.as_slice() else {
        return None;
    };
    Some(name.to_string())
}

fn moved_into_wild(trees: &[TokenTree], at: usize) -> Option<String> {
    let TokenTree::Ident(keyword) = &trees[at] else {
        return None;
    };
    if keyword != "let" {
        return None;
    }
    let TokenTree::Ident(wild) = trees.get(at + 1)? else {
        return None;
    };
    if wild != "_" {
        return None;
    }
    let TokenTree::Punct(eq) = trees.get(at + 2)? else {
        return None;
    };
    if eq.as_char() != '=' {
        return None;
    }
    let TokenTree::Ident(name) = trees.get(at + 3)? else {
        return None;
    };
    if name == "_" {
        return None;
    }
    match trees.get(at + 4) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis => None,
        Some(TokenTree::Punct(punct)) if punct.as_char() == '.' || punct.as_char() == ':' => None,
        _ => Some(name.to_string()),
    }
}

fn macro_tests(tokens: TokenStream, out: &mut Vec<TestFn>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for at in 0..trees.len() {
        if let TokenTree::Group(group) = &trees[at] {
            macro_tests(group.stream(), out);
        }
        if !test_attr_at(&trees, at) {
            continue;
        }
        let rest = &trees[at + 2..];
        let Some(fn_at) = rest
            .iter()
            .position(|tree| matches!(tree, TokenTree::Ident(word) if word == "fn"))
        else {
            continue;
        };
        let Some(body) = rest[fn_at..].iter().find_map(|tree| match tree {
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => Some(group),
            _ => None,
        }) else {
            continue;
        };
        let name = rest[fn_at + 1..]
            .iter()
            .take_while(|tree| !matches!(tree, TokenTree::Group(_)))
            .map(ToString::to_string)
            .collect();
        let stream = body.stream();
        let mut calls = Vec::new();
        token_calls(stream.clone(), &mut calls);
        out.push(TestFn {
            name,
            line: trees[at].span().start().line,
            bindings: token_bindings(stream.clone()),
            ended: token_ended(stream),
            calls,
        });
    }
}

#[derive(Default)]
struct Tests(Vec<TestFn>);

impl<'ast> Visit<'ast> for Tests {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if item.attrs.iter().any(is_test_attr) {
            let mut calls = Calls::default();
            calls.visit_block(&item.block);
            let mut ended = Ended::default();
            ended.visit_block(&item.block);
            self.0.push(TestFn {
                name: item.sig.ident.to_string(),
                line: item.sig.ident.span().start().line,
                bindings: item
                    .block
                    .stmts
                    .iter()
                    .enumerate()
                    .filter_map(|(at, stmt)| {
                        let (name, path) = guard_binding(stmt)?;
                        Some((at, name, path))
                    })
                    .collect(),
                ended: ended.0,
                calls: calls.0,
            });
        }
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        macro_tests(mac.tokens.clone(), &mut self.0);
    }
}

/// Every test function in `source`, including those a macro body writes.
/// A test attribute the syntax walk does not reach fails the scan, so a
/// test cannot hide from the guard in a shape this reader does not know.
fn test_fns(source: &str, path: &Path) -> Vec<TestFn> {
    let file = syn::parse_file(source)
        .unwrap_or_else(|err| panic!("{} does not parse: {err}", path.display()));
    let mut tests = Tests::default();
    tests.visit_file(&file);
    let attrs = count_test_attrs(source.parse().unwrap());
    assert_eq!(
        tests.0.len(),
        attrs,
        "{} has a test attribute the guard scan cannot read",
        path.display()
    );
    tests.0
}

fn source_calls(source: &str) -> Vec<String> {
    let file = syn::parse_file(source).unwrap_or_else(|err| panic!("{err}: {source}"));
    let mut calls = Calls::default();
    calls.visit_file(&file);
    calls.0
}

const LIB_GUARDS: &[&str] = &["test_env_read_lock", "test_env_lock", "env_guard"];

const LIB_WRITERS: &[&str] = &[
    "test_env_lock",
    "env_guard",
    "env_lock",
    "test_home",
    "review_env",
    "Shared::new",
    "isolated",
    "begin",
    "hold_env",
    "hud_env",
    "pack_aside",
];

const MUTATIONS: &[&str] = &["set_var", "remove_var", "set_current_dir"];

fn mutates_process_state(calls: &[String]) -> bool {
    calls.iter().any(|path| names_any(path, MUTATIONS))
}

fn unguarded(source: &str, path: &Path, guards: &[&str]) -> Vec<String> {
    test_fns(source, path)
        .into_iter()
        .filter(|test| !test.guarded(guards))
        .map(|test| test.name)
        .collect()
}

fn offender(path: &Path, test: &TestFn) -> String {
    format!("{}:{} `{}`", path.display(), test.line, test.name)
}

#[test]
fn process_global_test_state_is_guarded() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest.join("src");
    let mut missing = Vec::new();
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    for path in files {
        if path.starts_with(src.join("bin")) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for test in test_fns(&source, &path) {
            if !test.guarded(LIB_GUARDS) {
                missing.push(format!(
                    "{} is a unit test without a process-state guard",
                    offender(&path, &test)
                ));
            }
            if mutates_process_state(&test.calls) && !test.calls_any(LIB_WRITERS) {
                missing.push(format!(
                    "{} mutates process state in a unit test without the writer",
                    offender(&path, &test)
                ));
            }
        }
    }

    let mut bins = Vec::new();
    rust_files(&src.join("bin"), &mut bins);
    for path in bins {
        let source = std::fs::read_to_string(&path).unwrap();
        for test in test_fns(&source, &path) {
            if !test.guarded(&["process_lock"]) {
                missing.push(format!(
                    "{} is a binary unit test without the process-state writer",
                    offender(&path, &test)
                ));
            }
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
        let tests = test_fns(&source, &path);
        if path.starts_with(&hud) {
            for test in &tests {
                if !test.guarded(&["ljos_test_boot::env_read_lock"]) {
                    missing.push(format!(
                        "{} is a HUD unit test without the process-state reader",
                        offender(&path, test)
                    ));
                }
            }
        }
        if !mutates_process_state(&source_calls(&source)) {
            continue;
        }
        for test in &tests {
            if !test.guarded(&["ljos_test_boot::env_lock"]) {
                missing.push(format!(
                    "{} mutates process state but has a test without the shared writer",
                    offender(&path, test)
                ));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

fn probe(body: &str) -> String {
    format!("#[test]\nfn probe() {{\n    {body}\n    assert_eq!(1 + 1, 2);\n}}\n")
}

#[test]
fn a_comment_string_or_other_name_is_not_a_guard() {
    let path = Path::new("probe.rs");
    for body in [
        "// test_env_read_lock()",
        "// let _g = test_env_read_lock();",
        "/* let _g = crate::test_env_lock(); */",
        "/// env_guard()\n    let _x = 1;",
        "let _g = \"test_env_read_lock()\";",
        "let _g = r#\"let _g = env_guard();\"#;",
        "let _g = not_test_env_lock();",
        "let _g = test_env_read_lock_for_show();",
        "let test_env_read_lock = 1;",
        "let _g = test_env_read_lock;",
        "test_env_read_lock();",
        "let _ = test_env_read_lock();",
        "println!(\"{:?}\", test_env_read_lock());",
    ] {
        assert_eq!(
            unguarded(&probe(body), path, LIB_GUARDS),
            ["probe"],
            "{body}"
        );
    }
    for body in [
        "let _g = test_env_read_lock();",
        "let _env_read = crate::test_env_read_lock();",
        "let _g: EnvLock = crate::tests::env_guard();",
    ] {
        assert!(
            unguarded(&probe(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
    let early = probe("use std::path::Path;\n    let _g = env_guard();");
    assert!(unguarded(&early, path, LIB_GUARDS).is_empty());
    let late = probe("let a = 1;\n    let b = 2;\n    let _g = env_guard();");
    assert_eq!(unguarded(&late, path, LIB_GUARDS), ["probe"]);
}

#[test]
fn a_dropped_guard_does_not_count() {
    let path = Path::new("probe.rs");
    for body in [
        "let guard = test_env_lock();\n    drop(guard);",
        "let guard = crate::test_env_lock();\n    std::mem::drop(guard);",
        "let guard = test_env_lock();\n    let _ = guard;",
        "let _ = test_env_lock();",
    ] {
        assert_eq!(
            unguarded(&probe(body), path, LIB_GUARDS),
            ["probe"],
            "{body}"
        );
    }
    let held = probe("let guard = test_env_lock();");
    assert!(unguarded(&held, path, LIB_GUARDS).is_empty(), "{held}");
    let shape = |body: &str| {
        format!("macro_rules! cell {{ ($name:ident) => {{ #[test] fn $name() {{ {body} }} }}; }}")
    };
    let dropped = shape("let guard = test_env_lock(); drop(guard);");
    assert_eq!(unguarded(&dropped, path, LIB_GUARDS), ["$name"]);
    let held_mac = shape("let guard = test_env_lock();");
    assert!(unguarded(&held_mac, path, LIB_GUARDS).is_empty());
}

#[test]
fn a_test_written_by_a_macro_is_scanned() {
    let path = Path::new("probe.rs");
    let shape = |body: &str| {
        format!("macro_rules! cell {{ ($name:ident) => {{ #[test] fn $name() {{ {body} }} }}; }}")
    };
    let bare = shape("// env_guard()\n check();");
    assert_eq!(unguarded(&bare, path, LIB_GUARDS), ["$name"]);
    let held = shape("let _g = crate::tests::env_guard(); check();");
    assert!(unguarded(&held, path, LIB_GUARDS).is_empty());
}

#[test]
fn a_test_the_reader_cannot_reach_fails_the_scan() {
    let source = "impl Probe {\n    #[test]\n    fn probe() {}\n}\n";
    let caught = std::panic::catch_unwind(|| test_fns(source, Path::new("probe.rs")));
    assert!(caught.is_err());
}

#[test]
fn a_comment_or_string_is_not_the_writer() {
    let path = Path::new("probe.rs");
    for body in [
        "let _g = test_env_read_lock(); // begin(dir)\n    unsafe { std::env::set_var(\"A\", \"1\") };",
        "let _g = test_env_read_lock(); let _s = \"env_guard()\";\n    unsafe { std::env::set_var(\"A\", \"1\") };",
    ] {
        let tests = test_fns(&probe(body), path);
        assert!(mutates_process_state(&tests[0].calls), "{body}");
        assert!(!tests[0].calls_any(LIB_WRITERS), "{body}");
    }
    let held = probe("let _g = begin(dir);\n    unsafe { std::env::set_var(\"A\", \"1\") };");
    assert!(test_fns(&held, path)[0].calls_any(LIB_WRITERS));
}

#[test]
fn process_mutation_scan_catches_imports_and_aliases() {
    let wrap = |body: &str| format!("fn probe() {{ {body} }}");
    assert!(mutates_process_state(&source_calls(&wrap(
        "use std::env; env::set_var(\"NO_COLOR\", \"1\");"
    ))));
    assert!(mutates_process_state(&source_calls(&wrap(
        "use std::env::remove_var; remove_var (\"NO_COLOR\");"
    ))));
    assert!(mutates_process_state(&source_calls(&wrap(
        "use std::env as process_env; process_env::set_current_dir(\"/\");"
    ))));
    assert!(mutates_process_state(&source_calls(&wrap(
        "unsafe { assert!(std::env::set_var(\"A\", \"1\") == ()) };"
    ))));
    assert!(!mutates_process_state(&source_calls(&wrap(
        "// std::env::set_var(\"A\", \"1\");\n let _s = \"remove_var(x)\";"
    ))));
}

#[test]
fn test_body_scan_stops_at_the_next_test() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest.join("src/digest.rs");
    let source = std::fs::read_to_string(&path).unwrap();
    let first = &test_fns(&source, &path)[0];
    assert!(!mutates_process_state(&first.calls), "{}", first.name);
}
