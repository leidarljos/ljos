use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
// `ToTokens` reprints a function tail, including each macro's tokens, so the
// move pass reads the same stream a macro body already is. syn only
// re-exports the trait here.
use syn::__private::ToTokens;

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
    /// index, the name and the path called. A guard counts when that
    /// binding still holds the value at the end of the body.
    bindings: Vec<(usize, String, String)>,
    /// Top-level statement indices whose name is used by value while that
    /// binding is still in scope.
    ended: Vec<usize>,
    /// Every function, method and macro-argument call in the body.
    calls: Vec<String>,
}

impl TestFn {
    fn guarded(&self, guards: &[&str]) -> bool {
        self.bindings.iter().any(|(at, _, path)| {
            *at < GUARD_WITHIN && names_any(path, guards) && !self.ended.contains(at)
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
/// drops the value at once, so it is not a binding. A later by-value use
/// of that same binding ends it. A borrow or a method call does not, and
/// a bare `path();` never binds it.
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

/// The single name inside parentheses, groups and references.
/// `guard.field` and `a::guard` are not that name.
fn path_leaf(expr: &syn::Expr) -> Option<String> {
    let mut expr = expr;
    loop {
        expr = match expr {
            syn::Expr::Paren(inner) => &inner.expr,
            syn::Expr::Group(inner) => &inner.expr,
            syn::Expr::Reference(inner) => &inner.expr,
            syn::Expr::RawAddr(inner) => &inner.expr,
            syn::Expr::Path(path) => {
                if path.qself.is_some() || path.path.segments.len() != 1 {
                    return None;
                }
                return Some(path.path.segments[0].ident.to_string());
            }
            _ => return None,
        };
    }
}

/// A borrow hides the name from a by-value read. `path_leaf` still sees
/// through it, so `&guard` can be named without ending the binding.
fn hides_a_borrow(expr: &syn::Expr) -> bool {
    let mut expr = expr;
    loop {
        match expr {
            syn::Expr::Paren(inner) => expr = &inner.expr,
            syn::Expr::Group(inner) => expr = &inner.expr,
            syn::Expr::Reference(_) | syn::Expr::RawAddr(_) => return true,
            _ => return false,
        }
    }
}

fn value_name(expr: &syn::Expr) -> Option<String> {
    if hides_a_borrow(expr) {
        None
    } else {
        path_leaf(expr)
    }
}

fn local_name(stmt: &syn::Stmt) -> Option<String> {
    let syn::Stmt::Local(local) = stmt else {
        return None;
    };
    binding_name(&local.pat)
}

fn pattern_binds(pat: &syn::Pat, name: &str) -> bool {
    match pat {
        syn::Pat::Ident(pat) => {
            pat.ident == name
                || pat
                    .subpat
                    .as_ref()
                    .is_some_and(|(_, sub)| pattern_binds(sub, name))
        }
        syn::Pat::Or(pat) => pat.cases.iter().any(|case| pattern_binds(case, name)),
        syn::Pat::Paren(pat) => pattern_binds(&pat.pat, name),
        syn::Pat::Reference(pat) => pattern_binds(&pat.pat, name),
        syn::Pat::Type(pat) => pattern_binds(&pat.pat, name),
        syn::Pat::Slice(pat) => pat.elems.iter().any(|elem| pattern_binds(elem, name)),
        syn::Pat::Tuple(pat) => pat.elems.iter().any(|elem| pattern_binds(elem, name)),
        syn::Pat::TupleStruct(pat) => pat.elems.iter().any(|elem| pattern_binds(elem, name)),
        syn::Pat::Struct(pat) => pat
            .fields
            .iter()
            .any(|field| pattern_binds(&field.pat, name)),
        syn::Pat::Rest(_)
        | syn::Pat::Wild(_)
        | syn::Pat::Lit(_)
        | syn::Pat::Path(_)
        | syn::Pat::Range(_)
        | syn::Pat::Const(_)
        | syn::Pat::Macro(_)
        | syn::Pat::Verbatim(_) => false,
        _ => false,
    }
}

/// Statement indices of top-level `let` bindings whose name is used by
/// value after the let, in a scope that still sees that binding. Move
/// captures are read from the raw tokens first, before any place or
/// borrow is classified. Any token of that name in a move closure,
/// including the parameter list, ends the guard. A `let` of that name
/// does not. A use before the let, or under a later binding of the same
/// name, does not.
fn ended_bindings(stmts: &[syn::Stmt]) -> Vec<usize> {
    let mut ended = Vec::new();
    for (at, stmt) in stmts.iter().enumerate() {
        let Some(name) = local_name(stmt) else {
            continue;
        };
        let tail = &stmts[at + 1..];
        let tokens = tail_trees(tail);
        let statements: Vec<&[TokenTree]> = tokens.split(is_semi).collect();
        if moves_after(&statements, 0, &name) || stmts_consume(tail, &name, Use::Value, true) {
            ended.push(at);
        }
    }
    ended
}

fn tail_trees(stmts: &[syn::Stmt]) -> Vec<TokenTree> {
    let mut stream = TokenStream::new();
    for stmt in stmts {
        stream.extend(stmt.to_token_stream());
    }
    stream.into_iter().collect()
}

fn stmts_consume(stmts: &[syn::Stmt], name: &str, how: Use, mut live: bool) -> bool {
    for stmt in stmts {
        if stmt_uses(stmt, name, how, live) {
            return true;
        }
        if stmt_binds(stmt, name) {
            live = false;
        }
    }
    false
}

fn stmt_binds(stmt: &syn::Stmt, name: &str) -> bool {
    let syn::Stmt::Local(local) = stmt else {
        return false;
    };
    pattern_binds(&local.pat, name)
}

fn stmt_uses(stmt: &syn::Stmt, name: &str, how: Use, live: bool) -> bool {
    if !live {
        return false;
    }
    match stmt {
        syn::Stmt::Local(local) => local.init.as_ref().is_some_and(|init| {
            expr_uses(&init.expr, name, how, true)
                || init
                    .diverge
                    .as_ref()
                    .is_some_and(|(_, expr)| expr_uses(expr, name, how, true))
        }),
        syn::Stmt::Expr(expr, _) => expr_uses(expr, name, how, true),
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => false,
    }
}

fn block_uses(block: &syn::Block, name: &str, how: Use, live: bool) -> bool {
    stmts_consume(&block.stmts, name, how, live)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    Value,
    Place,
}

fn names_place(expr: &syn::Expr, name: &str, how: Use) -> bool {
    how == Use::Value && value_name(expr).as_deref() == Some(name)
}

fn compound_assign(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::AddAssign(_)
            | syn::BinOp::SubAssign(_)
            | syn::BinOp::MulAssign(_)
            | syn::BinOp::DivAssign(_)
            | syn::BinOp::RemAssign(_)
            | syn::BinOp::BitXorAssign(_)
            | syn::BinOp::BitAndAssign(_)
            | syn::BinOp::BitOrAssign(_)
            | syn::BinOp::ShlAssign(_)
            | syn::BinOp::ShrAssign(_)
    )
}

fn expr_uses(expr: &syn::Expr, name: &str, how: Use, live: bool) -> bool {
    if !live {
        return false;
    }
    if names_place(expr, name, how) {
        return true;
    }
    match expr {
        syn::Expr::Paren(inner) => expr_uses(&inner.expr, name, how, live),
        syn::Expr::Group(inner) => expr_uses(&inner.expr, name, how, live),
        syn::Expr::Reference(inner) => expr_uses(&inner.expr, name, Use::Place, live),
        syn::Expr::RawAddr(inner) => expr_uses(&inner.expr, name, Use::Place, live),
        syn::Expr::Path(_) => false,
        syn::Expr::Field(field) => expr_uses(&field.base, name, Use::Place, live),
        syn::Expr::Index(index) => {
            expr_uses(&index.expr, name, Use::Place, live)
                || expr_uses(&index.index, name, Use::Value, live)
        }
        syn::Expr::Unary(unary) => {
            let next = if matches!(unary.op, syn::UnOp::Deref(_)) {
                Use::Place
            } else {
                Use::Value
            };
            expr_uses(&unary.expr, name, next, live)
        }
        syn::Expr::MethodCall(call) => {
            expr_uses(&call.receiver, name, Use::Place, live)
                || call
                    .args
                    .iter()
                    .any(|arg| expr_uses(arg, name, Use::Value, live))
        }
        syn::Expr::Call(call) => {
            expr_uses(&call.func, name, Use::Value, live)
                || call
                    .args
                    .iter()
                    .any(|arg| expr_uses(arg, name, Use::Value, live))
        }
        syn::Expr::Assign(assign) => {
            value_name(&assign.left).as_deref() == Some(name)
                || expr_uses(&assign.left, name, Use::Place, live)
                || expr_uses(&assign.right, name, Use::Value, live)
        }
        syn::Expr::Binary(bin) => {
            let left = if compound_assign(&bin.op) {
                Use::Place
            } else {
                Use::Value
            };
            expr_uses(&bin.left, name, left, live) || expr_uses(&bin.right, name, Use::Value, live)
        }
        syn::Expr::Array(array) => array
            .elems
            .iter()
            .any(|elem| expr_uses(elem, name, Use::Value, live)),
        syn::Expr::Tuple(tuple) => tuple
            .elems
            .iter()
            .any(|elem| expr_uses(elem, name, Use::Value, live)),
        syn::Expr::Repeat(repeat) => {
            expr_uses(&repeat.expr, name, Use::Value, live)
                || expr_uses(&repeat.len, name, Use::Value, live)
        }
        syn::Expr::Struct(expr) => {
            expr.fields
                .iter()
                .any(|field| expr_uses(&field.expr, name, Use::Value, live))
                || expr
                    .rest
                    .as_ref()
                    .is_some_and(|rest| expr_uses(rest, name, Use::Value, live))
        }
        syn::Expr::Range(range) => {
            range
                .start
                .as_ref()
                .is_some_and(|start| expr_uses(start, name, Use::Value, live))
                || range
                    .end
                    .as_ref()
                    .is_some_and(|end| expr_uses(end, name, Use::Value, live))
        }
        syn::Expr::Cast(cast) => expr_uses(&cast.expr, name, Use::Value, live),
        syn::Expr::Try(try_expr) => expr_uses(&try_expr.expr, name, Use::Value, live),
        syn::Expr::Await(await_expr) => expr_uses(&await_expr.base, name, Use::Value, live),
        syn::Expr::Let(let_expr) => expr_uses(&let_expr.expr, name, Use::Value, live),
        syn::Expr::Return(ret) => ret
            .expr
            .as_ref()
            .is_some_and(|expr| expr_uses(expr, name, Use::Value, live)),
        syn::Expr::Break(brk) => brk
            .expr
            .as_ref()
            .is_some_and(|expr| expr_uses(expr, name, Use::Value, live)),
        syn::Expr::Yield(yield_expr) => yield_expr
            .expr
            .as_ref()
            .is_some_and(|expr| expr_uses(expr, name, Use::Value, live)),
        syn::Expr::Block(block) => block_uses(&block.block, name, how, live),
        syn::Expr::Unsafe(block) => block_uses(&block.block, name, how, live),
        syn::Expr::Loop(block) => block_uses(&block.body, name, how, live),
        syn::Expr::TryBlock(block) => block_uses(&block.block, name, how, live),
        syn::Expr::Async(block) => block_uses(&block.block, name, Use::Value, live),
        syn::Expr::Const(_) | syn::Expr::Continue(_) | syn::Expr::Infer(_) | syn::Expr::Lit(_) => {
            false
        }
        syn::Expr::Macro(_) | syn::Expr::Verbatim(_) => false,
        syn::Expr::If(expr) => if_uses(expr, name, how, live),
        syn::Expr::While(expr) => while_uses(expr, name, how, live),
        syn::Expr::ForLoop(expr) => {
            expr_uses(&expr.expr, name, Use::Value, live)
                || block_uses(
                    &expr.body,
                    name,
                    how,
                    live && !pattern_binds(&expr.pat, name),
                )
        }
        syn::Expr::Match(expr) => match_uses(expr, name, live),
        syn::Expr::Closure(expr) => {
            let body_live = live && !expr.inputs.iter().any(|pat| pattern_binds(pat, name));
            expr_uses(&expr.body, name, Use::Value, body_live)
        }
        _ => false,
    }
}

/// A let chain binds from left to right. A pattern hides that name from
/// later `&&` operands. The pair is whether the outer name was used, and
/// whether a pattern bound it.
fn cond_chain(expr: &syn::Expr, name: &str, live: bool) -> (bool, bool) {
    match expr {
        syn::Expr::Paren(inner) => cond_chain(&inner.expr, name, live),
        syn::Expr::Group(inner) => cond_chain(&inner.expr, name, live),
        syn::Expr::Binary(bin) if matches!(bin.op, syn::BinOp::And(_)) => {
            let (left_used, left_shadow) = cond_chain(&bin.left, name, live);
            let (right_used, right_shadow) = cond_chain(&bin.right, name, live && !left_shadow);
            (left_used || right_used, left_shadow || right_shadow)
        }
        syn::Expr::Let(let_expr) => (
            expr_uses(&let_expr.expr, name, Use::Value, live),
            pattern_binds(&let_expr.pat, name),
        ),
        other => (expr_uses(other, name, Use::Value, live), false),
    }
}

fn if_uses(expr: &syn::ExprIf, name: &str, how: Use, live: bool) -> bool {
    let (used, shadowed) = cond_chain(&expr.cond, name, live);
    used || block_uses(&expr.then_branch, name, how, live && !shadowed)
        || expr
            .else_branch
            .as_ref()
            .is_some_and(|(_, else_expr)| expr_uses(else_expr, name, Use::Value, live))
}

fn while_uses(expr: &syn::ExprWhile, name: &str, how: Use, live: bool) -> bool {
    let (used, shadowed) = cond_chain(&expr.cond, name, live);
    used || block_uses(&expr.body, name, how, live && !shadowed)
}

fn match_uses(expr: &syn::ExprMatch, name: &str, live: bool) -> bool {
    if expr_uses(&expr.expr, name, Use::Value, live) {
        return true;
    }
    expr.arms.iter().any(|arm| {
        let arm_live = live && !pattern_binds(&arm.pat, name);
        arm.guard
            .as_ref()
            .is_some_and(|(_, cond)| cond_chain(cond, name, arm_live).0)
            || expr_uses(&arm.body, name, Use::Value, arm_live)
    })
}

fn is_semi(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ';')
}

fn is_eq(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == '=')
}

fn is_colon(tree: Option<&TokenTree>) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == ':')
}

/// The same binding read from a macro body's tokens, where `$name` and
/// other fragments keep syn from parsing a statement.
fn token_bindings(body: TokenStream) -> Vec<(usize, String, String)> {
    let trees: Vec<TokenTree> = body.into_iter().collect();
    let statements = trees.split(is_semi);
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
                TokenTree::Punct(punct) => punct.as_char() == ':',
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

/// Statement indices in a macro body whose `let` name is used by value
/// while that binding is still in scope. Semicolons inside braces stay
/// with that block, matching `token_bindings`. Move captures are a
/// separate pass over those same tokens.
fn token_ended(body: TokenStream) -> Vec<usize> {
    let trees: Vec<TokenTree> = body.into_iter().collect();
    let statements: Vec<&[TokenTree]> = trees.split(is_semi).collect();
    let mut ended = Vec::new();
    for (at, statement) in statements.iter().enumerate() {
        let Some(name) = bound_simple(statement) else {
            continue;
        };
        if moves_after(&statements, at + 1, &name) || rest_moves(&statements, at + 1, &name) {
            ended.push(at);
        }
    }
    ended
}

fn bound_simple(statement: &[TokenTree]) -> Option<String> {
    let (TokenTree::Ident(keyword), rest) = statement.split_first()? else {
        return None;
    };
    if keyword != "let" {
        return None;
    }
    let eq = rest.iter().position(is_eq)?;
    simple_name(&rest[..eq])
}

fn simple_name(pattern: &[TokenTree]) -> Option<String> {
    let mut index = 0;
    while index < pattern.len() {
        match &pattern[index] {
            TokenTree::Ident(ident) if ident == "mut" || ident == "ref" => index += 1,
            TokenTree::Ident(ident) => {
                let typed = is_colon(pattern.get(index + 1)) && !is_colon(pattern.get(index + 2));
                let alone = index + 1 == pattern.len();
                let binding = matches!(
                    pattern.get(index + 1),
                    Some(TokenTree::Punct(punct)) if punct.as_char() == '@'
                );
                if typed || alone || binding {
                    let name = ident.to_string();
                    if name == "_" {
                        return None;
                    }
                    return Some(name);
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

fn rest_moves(statements: &[&[TokenTree]], from: usize, name: &str) -> bool {
    let mut live = true;
    for statement in &statements[from..] {
        if live && walk_tokens(statement, name) {
            return true;
        }
        if statement_shadows(statement, name) {
            live = false;
        }
    }
    false
}

fn statement_shadows(trees: &[TokenTree], name: &str) -> bool {
    let Some(TokenTree::Ident(keyword)) = trees.first() else {
        return false;
    };
    if keyword != "let" {
        return false;
    }
    let Some(eq) = trees.iter().position(is_eq) else {
        return false;
    };
    simple_name(&trees[1..eq]).is_some_and(|got| got == name)
}

fn is_pipe(tree: Option<&TokenTree>) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == '|')
}

fn is_end(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(punct) if matches!(punct.as_char(), ',' | ';'))
}

/// `move` as a keyword. `name: move || ...` counts. `::move` and `.move` do not.
fn keyword_move(trees: &[TokenTree], index: usize) -> bool {
    if !matches!(trees.get(index), Some(TokenTree::Ident(word)) if word == "move") {
        return false;
    }
    match index.checked_sub(1).and_then(|at| trees.get(at)) {
        Some(TokenTree::Punct(punct)) if punct.as_char() == '.' => false,
        Some(TokenTree::Punct(punct)) if punct.as_char() == ':' => {
            !is_colon(index.checked_sub(2).and_then(|at| trees.get(at)))
        }
        _ => true,
    }
}

/// Where the move region ends. A brace is the body. Otherwise the region
/// runs to a comma, a semicolon, or the end of this group. Callers read
/// every ident from the token after `move` through this end.
fn closure_end(trees: &[TokenTree], move_at: usize) -> usize {
    let mut at = move_at + 1;
    if is_pipe(trees.get(at)) {
        at += 1;
        while at < trees.len() && !is_pipe(trees.get(at)) {
            at += 1;
        }
        if at < trees.len() {
            at += 1;
        }
    }
    match trees.get(at) {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => at + 1,
        _ => trees[at..]
            .iter()
            .position(is_end)
            .map_or(trees.len(), |rel| at + rel),
    }
}

/// An ident of the live `name`. A `let` of that name hides the rest of its
/// block. The initializer can still name the outer guard.
fn mentions(trees: &[TokenTree], name: &str) -> bool {
    let mut live = true;
    for statement in trees.split(is_semi) {
        if !live {
            continue;
        }
        if statement_shadows(statement, name) {
            if statement
                .iter()
                .position(is_eq)
                .is_some_and(|eq| mentions(&statement[eq + 1..], name))
            {
                return true;
            }
            live = false;
            continue;
        }
        if statement.iter().any(|tree| token_mentions(tree, name)) {
            return true;
        }
    }
    false
}

fn token_mentions(tree: &TokenTree, name: &str) -> bool {
    match tree {
        TokenTree::Ident(ident) => ident == name,
        TokenTree::Group(group) => {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            mentions(&inner, name)
        }
        _ => false,
    }
}

/// A `move` or `async move` in these statements captures the live `name`.
/// Any token of that name in the closure, including a parameter, counts.
/// Groups are opened, so a macro token tree counts. Runs before borrows.
fn moves_after(statements: &[&[TokenTree]], from: usize, name: &str) -> bool {
    let mut live = true;
    for statement in &statements[from..] {
        if live && stmt_moves_capture(statement, name) {
            return true;
        }
        if statement_shadows(statement, name) {
            live = false;
        }
    }
    false
}

fn stmt_moves_capture(trees: &[TokenTree], name: &str) -> bool {
    let mut index = 0;
    while index < trees.len() {
        if keyword_move(trees, index) {
            let end = closure_end(trees, index);
            if mentions(&trees[index + 1..end], name) {
                return true;
            }
            index = end.max(index + 1);
            continue;
        }
        if let TokenTree::Group(group) = &trees[index] {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            let nested: Vec<&[TokenTree]> = inner.split(is_semi).collect();
            if moves_after(&nested, 0, name) {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn walk_tokens(trees: &[TokenTree], name: &str) -> bool {
    let mut index = 0;
    while index < trees.len() {
        if keyword_move(trees, index) {
            let end = closure_end(trees, index);
            index = end.max(index + 1);
            continue;
        }
        match &trees[index] {
            TokenTree::Group(group) => {
                let inner: Vec<TokenTree> = group.stream().into_iter().collect();
                let moved = if group.delimiter() == Delimiter::Brace {
                    brace_moves(&inner, name)
                } else if group_is_borrowed(trees, index) {
                    moves_under_borrow(&inner, name)
                } else {
                    walk_tokens(&inner, name)
                };
                if moved {
                    return true;
                }
            }
            TokenTree::Ident(_) => {
                if ident_moves(trees, index, name) {
                    return true;
                }
            }
            _ => {}
        }
        index += 1;
    }
    false
}

/// `&(guard)` keeps the binding. The `&` is outside the parentheses, so
/// the name inside is still a place, while `&(foo(guard))` is not.
fn group_is_borrowed(trees: &[TokenTree], index: usize) -> bool {
    if index == 0 {
        return false;
    }
    match &trees[index - 1] {
        TokenTree::Punct(punct) if punct.as_char() == '&' => true,
        TokenTree::Ident(ident) if ident == "mut" || ident == "const" || ident == "raw" => {
            let mut at = index;
            while at > 0 {
                at -= 1;
                match &trees[at] {
                    TokenTree::Ident(word) if word == "mut" || word == "const" || word == "raw" => {
                    }
                    TokenTree::Punct(punct) if punct.as_char() == '&' => return true,
                    _ => return false,
                }
            }
            false
        }
        _ => false,
    }
}

fn moves_under_borrow(trees: &[TokenTree], name: &str) -> bool {
    if let [TokenTree::Group(group)] = trees {
        if group.delimiter() != Delimiter::Brace {
            let inner: Vec<TokenTree> = group.stream().into_iter().collect();
            return moves_under_borrow(&inner, name);
        }
    }
    walk_tokens(skip_place_head(trees), name)
}

fn skip_place_head(trees: &[TokenTree]) -> &[TokenTree] {
    let Some(TokenTree::Ident(_)) = trees.first() else {
        return trees;
    };
    let mut index = 0;
    while index < trees.len() {
        match &trees[index] {
            TokenTree::Ident(word) if word == "move" => break,
            TokenTree::Ident(_) => index += 1,
            TokenTree::Punct(punct) if punct.as_char() == ':' => index += 1,
            _ => break,
        }
    }
    if matches!(&trees.get(index), Some(TokenTree::Punct(punct)) if punct.as_char() == '.') {
        index += 1;
        if matches!(trees.get(index), Some(TokenTree::Ident(_))) {
            index += 1;
        }
    }
    &trees[index..]
}

fn brace_moves(trees: &[TokenTree], name: &str) -> bool {
    rest_moves(&trees.split(is_semi).collect::<Vec<_>>(), 0, name)
}

fn ident_moves(trees: &[TokenTree], index: usize, name: &str) -> bool {
    let TokenTree::Ident(ident) = &trees[index] else {
        return false;
    };
    ident == name && !next_keeps(trees, index) && !prev_keeps(trees, index)
}

fn next_keeps(trees: &[TokenTree], index: usize) -> bool {
    match trees.get(index + 1) {
        Some(TokenTree::Punct(punct)) => {
            let ch = punct.as_char();
            ch == '.' || ch == ':'
        }
        Some(TokenTree::Group(group)) => group.delimiter() == Delimiter::Bracket,
        _ => false,
    }
}

fn borrowed_keyword(trees: &[TokenTree], index: usize) -> bool {
    let mut at = index;
    while at > 0 {
        at -= 1;
        match &trees[at] {
            TokenTree::Ident(ident) if ident == "mut" || ident == "const" || ident == "raw" => {}
            TokenTree::Ident(ident) if ident == "let" || ident == "ref" => return true,
            TokenTree::Punct(punct) if punct.as_char() == '&' => return true,
            _ => return false,
        }
    }
    false
}

fn prev_keeps(trees: &[TokenTree], index: usize) -> bool {
    if index == 0 {
        return false;
    }
    match &trees[index - 1] {
        TokenTree::Punct(punct) => {
            let ch = punct.as_char();
            ch == '.' || ch == ':' || ch == '&'
        }
        TokenTree::Ident(ident) if ident == "let" || ident == "ref" => true,
        TokenTree::Ident(ident) if ident == "mut" || ident == "const" || ident == "raw" => {
            borrowed_keyword(trees, index)
        }
        TokenTree::Ident(_) => false,
        _ => false,
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
                ended: ended_bindings(&item.block.stmts),
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

fn cell(body: &str) -> String {
    format!("macro_rules! cell {{ ($name:ident) => {{ #[test] fn $name() {{ {body} }} }}; }}")
}

#[test]
fn a_wrapped_drop_does_not_count() {
    let path = Path::new("probe.rs");
    for body in [
        "let guard = test_env_lock();\n    drop((guard));",
        "let guard = test_env_lock();\n    let _ = (guard);",
        "let guard = test_env_lock();\n    drop(((guard)));",
        "let guard = test_env_lock();\n    std::mem::drop((guard));",
        "let guard = test_env_lock();\n    drop({ guard });",
        "let guard = test_env_lock();\n    let _ = { guard };",
    ] {
        assert_eq!(
            unguarded(&probe(body), path, LIB_GUARDS),
            ["probe"],
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock();\n    let _borrow = &guard;",
        "let guard = test_env_lock();\n    let _borrow = &mut guard;",
        "let guard = test_env_lock();\n    let _borrow = &(guard);",
        "let guard = test_env_lock();\n    let _borrow = &((guard));",
        "let guard = test_env_lock();\n    drop(&guard);",
        "let guard = test_env_lock();\n    let _ = &guard;",
        "let guard = test_env_lock();\n    guard.hold();",
        "let guard = test_env_lock();\n    (guard).hold();",
        "let guard = test_env_lock();\n    (&guard).hold();",
    ] {
        assert!(
            unguarded(&probe(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock(); drop((guard));",
        "let guard = test_env_lock(); let _ = (guard);",
    ] {
        assert_eq!(
            unguarded(&cell(body), path, LIB_GUARDS),
            ["$name"],
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock(); let _borrow = &guard;",
        "let guard = test_env_lock(); guard.hold();",
        "let guard = test_env_lock(); let _ = &(guard);",
    ] {
        assert!(
            unguarded(&cell(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
}

#[test]
fn a_moved_guard_does_not_count() {
    let path = Path::new("probe.rs");
    for body in [
        "let guard = test_env_lock();\n    let moved = guard;\n    drop(moved);",
        "let guard = test_env_lock();\n    let moved = guard;",
        "let guard = test_env_lock();\n    let moved = (guard);",
        "let guard = test_env_lock();\n    { let moved = guard; }",
        "let guard = test_env_lock();\n    let guard = guard;",
    ] {
        assert_eq!(
            unguarded(&probe(body), path, LIB_GUARDS),
            ["probe"],
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock();\n    let moved = &guard;",
        "let guard = test_env_lock();\n    let moved = &(guard);",
        "let guard = test_env_lock();\n    let guard = 1;",
        "let guard = test_env_lock();\n    let guard = 1;\n    drop(guard);",
    ] {
        assert!(
            unguarded(&probe(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
    let moved = cell("let guard = test_env_lock(); let moved = guard; drop(moved);");
    assert_eq!(unguarded(&moved, path, LIB_GUARDS), ["$name"]);
    let borrowed = cell("let guard = test_env_lock(); let moved = &guard;");
    assert!(unguarded(&borrowed, path, LIB_GUARDS).is_empty());
}

#[test]
fn a_move_capture_ends_the_guard() {
    let path = Path::new("probe.rs");
    for body in [
        "let guard = test_env_lock();\n    let release = move || helper(&guard);\n    drop(release);",
        "let guard = test_env_lock();\n    let release = async move { helper(&guard) };\n    drop(release);",
        "let guard = test_env_lock();\n    let release = move || assert!(guard.ready());",
        "let guard = test_env_lock();\n    let release = move || format!(\"{}\", guard);",
        "let guard = test_env_lock();\n    let release = move || (outer!(inner!(guard)));",
        "let guard = test_env_lock();\n    let release = move || vec![guard];",
        "let guard = test_env_lock();\n    let release = async move { assert!(guard.ready()) };",
        "let guard = test_env_lock();\n    let release = &(move || helper(&guard));",
        "let guard = test_env_lock();\n    let release = (move || helper(&guard));",
        "let guard = test_env_lock();\n    x.spawn(move || helper(&guard));",
        "let guard = test_env_lock();\n    let _job = Job { task: move || helper(&guard) };",
        "let guard = test_env_lock();\n    let release = move || { || helper(&guard) };",
        "let guard = test_env_lock();\n    let release = async move { foo!(guard) };",
        "let guard = test_env_lock();\n    let release = Some(move || helper(&guard));",
        "let guard = test_env_lock();\n    let release = vec![move || helper(&guard)];",
        "let guard = test_env_lock();\n    let release = move |S { guard: other }| helper(&guard);",
        "let guard = test_env_lock();\n    let release = move |(guard, _)| ();",
        "let guard = test_env_lock();\n    let release = move |guard| helper(&guard);",
        "let guard = test_env_lock();\n    let release = move |guard| assert!(guard.ready());",
        "let guard = test_env_lock();\n    let release = &(move |guard| helper(&guard));",
        "let guard = test_env_lock();\n    x.spawn(move |guard| guard);",
    ] {
        assert_eq!(
            unguarded(&probe(body), path, LIB_GUARDS),
            ["probe"],
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock();\n    let release = || helper(&guard);\n    drop(release);",
        "let guard = test_env_lock();\n    let release = move || helper(&other);",
        "let guard = test_env_lock();\n    let release = move || { let guard = 1;\n        helper(&guard) };",
        "let guard = test_env_lock();\n    let release = move || { let guard = 1;\n        assert!(guard.ready()) };",
        "let guard = test_env_lock();\n    let release = async { helper(&guard) };",
        "let guard = test_env_lock();\n    let release = async move { let guard = 1;\n        format!(\"{}\", guard) };",
    ] {
        assert!(
            unguarded(&probe(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock(); let release = move || assert!(guard.ready());",
        "let guard = test_env_lock(); let release = move || format!(\"{}\", guard);",
        "let guard = test_env_lock(); let release = move || (outer!(inner!(guard)));",
        "let guard = test_env_lock(); let release = move || vec![guard];",
        "let guard = test_env_lock(); let release = async move { assert!(guard.ready()) };",
        "let guard = test_env_lock(); let release = &(move || helper(&guard));",
        "let guard = test_env_lock(); let release = (move || helper(&guard));",
        "let guard = test_env_lock(); x.spawn(move || helper(&guard));",
        "let guard = test_env_lock(); let _job = Job { task: move || helper(&guard) };",
        "let guard = test_env_lock(); let release = move || { || helper(&guard) };",
        "let guard = test_env_lock(); let release = async move { foo!(guard) };",
        "let guard = test_env_lock(); let release = Some(move || helper(&guard));",
        "let guard = test_env_lock(); let release = vec![move || helper(&guard)];",
        "let guard = test_env_lock(); let release = move |S { guard: other }| helper(&guard);",
        "let guard = test_env_lock(); let release = move |(guard, _)| ();",
        "let guard = test_env_lock(); let release = move |guard| assert!(guard.ready());",
        "let guard = test_env_lock(); let release = &(move |guard| helper(&guard));",
        "let guard = test_env_lock(); x.spawn(move |guard| guard);",
    ] {
        assert_eq!(
            unguarded(&cell(body), path, LIB_GUARDS),
            ["$name"],
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock(); let release = move || { let guard = 1; assert!(guard.ready()) };",
        "let guard = test_env_lock(); let release = move || helper(&other);",
    ] {
        assert!(
            unguarded(&cell(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
}

#[test]
fn a_let_chain_keeps_its_own_binding() {
    let path = Path::new("probe.rs");
    for body in [
        "let guard = test_env_lock();\n    let value = Some(1);\n    match value {\n        _ if let Some(other) = value && { drop(guard); true } => {}\n    }",
        "let guard = test_env_lock();\n    let value = Some(1);\n    if let Some(other) = value && { drop(guard); true } {}",
        "let guard = test_env_lock();\n    let value = Some(1);\n    while let Some(other) = value && { drop(guard); true } {}",
    ] {
        assert_eq!(
            unguarded(&probe(body), path, LIB_GUARDS),
            ["probe"],
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock();\n    let value = Some(1);\n    match value {\n        _ if let Some(guard) = value && { drop(guard); true } => {}\n    }",
        "let guard = test_env_lock();\n    let value = Some(1);\n    if let Some(guard) = value && { drop(guard); true } {}",
        "let guard = test_env_lock();\n    let value = Some(1);\n    while let Some(guard) = value && { drop(guard); true } {}",
    ] {
        assert!(
            unguarded(&probe(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
}

#[test]
fn a_guard_name_follows_scope() {
    let path = Path::new("probe.rs");
    for body in [
        "let guard = test_env_lock();\n    { drop(guard); }",
        "let guard = test_env_lock();\n    { let _ = (guard); }",
        "let guard = test_env_lock();\n    { drop((guard)); }",
        "let guard = test_env_lock();\n    { drop(guard);\n        let guard = 1; }",
    ] {
        assert_eq!(
            unguarded(&probe(body), path, LIB_GUARDS),
            ["probe"],
            "{body}"
        );
    }
    for body in [
        "drop(guard);\n    let guard = test_env_lock();",
        "let _ = (other);\n    let guard = test_env_lock();",
        "let guard = test_env_lock();\n    { let guard = 1;\n        drop(guard); }",
        "let guard = test_env_lock();\n    { let guard = 1;\n        let _ = (guard); }",
        "let guard = test_env_lock();\n    { let guard = 1;\n        let moved = guard;\n        drop(moved); }",
        "{ let guard = 1;\n        drop((guard)); }\n    let guard = test_env_lock();",
        "{ drop(guard); }\n    let guard = test_env_lock();",
    ] {
        assert!(
            unguarded(&probe(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
    for body in [
        "let guard = test_env_lock(); { drop(guard); }",
        "let guard = test_env_lock(); { drop((guard)); }",
    ] {
        assert_eq!(
            unguarded(&cell(body), path, LIB_GUARDS),
            ["$name"],
            "{body}"
        );
    }
    for body in [
        "drop(guard); let guard = test_env_lock();",
        "let guard = test_env_lock(); { let guard = 1; drop(guard); }",
        "let guard = test_env_lock(); let guard = 1; drop(guard);",
        "{ let guard = 1; drop((guard)); }; let guard = test_env_lock();",
    ] {
        assert!(
            unguarded(&cell(body), path, LIB_GUARDS).is_empty(),
            "{body}"
        );
    }
}

#[test]
fn path_leaf_unwraps_parens_groups_and_references() {
    let paren: syn::Expr = syn::parse_str("(guard)").unwrap();
    let nested: syn::Expr = syn::parse_str("((guard))").unwrap();
    let reference: syn::Expr = syn::parse_str("&mut (guard)").unwrap();
    let grouped = TokenTree::Group(proc_macro2::Group::new(
        Delimiter::None,
        "guard".parse().unwrap(),
    ));
    let group: syn::Expr = syn::parse2(std::iter::once(grouped).collect()).unwrap();
    assert_eq!(path_leaf(&paren).as_deref(), Some("guard"));
    assert_eq!(path_leaf(&nested).as_deref(), Some("guard"));
    assert_eq!(path_leaf(&reference).as_deref(), Some("guard"));
    assert_eq!(path_leaf(&group).as_deref(), Some("guard"));
    assert!(hides_a_borrow(&reference));
    assert!(value_name(&reference).is_none());
    assert_eq!(value_name(&paren).as_deref(), Some("guard"));
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
