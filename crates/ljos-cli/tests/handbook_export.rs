//! `ljos handbook` against a real tracker and a real pack.
//!
//! The export is opt-in: a lesson, preference or rule marked `--public`, a
//! ticket tagged `public` and a workflow file with `#+public: yes`. Each test
//! drives the binary.

ljos_test_boot::harness!();

#[path = "common/tools.rs"]
mod tools;

#[path = "common/hand_world.rs"]
mod hand_world;

use std::path::{Path, PathBuf};
use std::process::Command;

use hand_world::{dump, on_path, ready, World};

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn handbook(world: &World, extra: &[&str]) -> std::process::Output {
    let mut args = vec!["handbook", "-p", "kiln"];
    args.extend(extra.iter().copied());
    world.ljos(&args)
}

#[test]
fn an_unmarked_lesson_is_not_exported() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&["remember", "-p", "kiln", "Keep the bench level."]);
    world.ok(&["remember", "-p", "kiln", "--public", "Wax the door hinge."]);
    let page = world.ok(&["handbook", "-p", "kiln"]);
    assert!(page.contains("Wax the door hinge."), "{page}");
    assert!(!page.contains("Keep the bench level."), "{page}");

    let only_plain = World::open();
    only_plain.ok(&["remember", "-p", "kiln", "Keep the bench level."]);
    let none = handbook(&only_plain, &[]);
    assert!(!none.status.success(), "{}", dump(&none));
    assert!(
        stderr(&none).contains("no items marked public"),
        "{}",
        dump(&none)
    );
    assert!(none.stdout.is_empty(), "{}", dump(&none));
}

#[test]
fn a_marked_lesson_that_names_the_editor_refuses_and_writes_nothing() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    world.ok(&["remember", "-p", "kiln", "--public", "Cursor is great."]);

    let out = world.root.join("book.org");
    let refused = handbook(&world, &["--out", out.to_str().unwrap()]);
    let said = dump(&refused);
    assert!(!refused.status.success(), "{said}");
    let err = stderr(&refused);
    assert!(err.contains("handbook: refused lesson "), "{err}");
    assert!(err.contains("nothing was written"), "{err}");
    assert!(!err.contains("Cursor"), "{err}");
    assert!(refused.stdout.is_empty(), "{said}");
    assert!(!out.exists(), "a refused export wrote a file");

    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&out, "kept\n").unwrap();
    let again = handbook(&world, &["--out", out.to_str().unwrap()]);
    assert!(!again.status.success(), "{}", dump(&again));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "kept\n");

    let to_stdout = handbook(&world, &["--out", "-"]);
    assert!(!to_stdout.status.success(), "{}", dump(&to_stdout));
    assert!(to_stdout.stdout.is_empty(), "{}", dump(&to_stdout));
    assert!(!stderr(&to_stdout).contains("Cursor"));
}

#[test]
fn a_marked_clean_lesson_exports() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    let out = world.root.join("book.org");
    world.ok(&["handbook", "-p", "kiln", "--out", out.to_str().unwrap()]);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("- Publish the signed archive.\n"), "{text}");
    assert!(text.starts_with("#+title: Project handbook\n"), "{text}");
}

#[test]
fn the_title_and_headings_carry_no_unmarked_text() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    world.ticket("kiln", "kiln-root", "Cut the release", None, "public");
    world.ticket("kiln", "kiln-secret", "Orchard budget plan", None, "");
    world.ticket(
        "kiln",
        "kiln-open",
        "Quince tasting notes",
        Some("Quince body."),
        "",
    );
    let dir = world.root.join(".ljos/workflows");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        dir.join("walnut.org"),
        "#+title: Walnut supplier calls\n\nCall them.\n",
    )
    .unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        dir.join("release.org"),
        "#+public: yes\n#+title: Release train\n\nPublish it.\n",
    )
    .unwrap();
    let page = world.ok(&[
        "handbook",
        "-p",
        "kiln",
        "--tree",
        "kiln-root",
        "--workflows",
        dir.to_str().unwrap(),
    ]);
    assert!(page.starts_with("#+title: Project handbook\n"), "{page}");
    for hidden in [
        "Orchard",
        "Quince",
        "Walnut",
        "walnut",
        "kiln-secret",
        "kiln-open",
        "kiln handbook",
    ] {
        assert!(!page.contains(hidden), "{hidden} leaked\n{page}");
    }
    assert!(page.contains("Cut the release"), "{page}");
    assert!(page.contains("release.org"), "{page}");

    let bare = handbook(&world, &["--title", "Orchard notes"]);
    assert!(bare.status.success(), "{}", dump(&bare));
    assert!(String::from_utf8_lossy(&bare.stdout).starts_with("#+title: Orchard notes\n"));

    let bad = handbook(&world, &["--title", "Notes for /home/ada"]);
    assert!(!bad.status.success(), "{}", dump(&bad));
    assert!(stderr(&bad).contains("refused --title"), "{}", dump(&bad));
    assert!(!stderr(&bad).contains("ada"), "{}", dump(&bad));
    assert!(bad.stdout.is_empty());
}

#[test]
fn tickets_are_marked_with_a_tag_and_an_unmarked_blocker_is_external() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ticket(
        "kiln",
        "kiln-root",
        "Cut the release",
        Some("Ship it."),
        "public",
    );
    world.ticket("kiln", "kiln-plain", "Quiet work", Some("Quiet body."), "");
    let path = vissue_core::Layout::new(&world.root, "day").project_issues_path("kiln");
    let org = std::fs::read_to_string(&path).unwrap();
    let org = org.replace(
        ":ID:         kiln-root",
        ":ID:         kiln-root\n:BLOCKED_BY: kiln-plain",
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&path, org).unwrap();

    let page = world.ok(&["handbook", "-p", "kiln", "--tree", "kiln-root"]);
    assert!(page.contains("Cut the release"), "{page}");
    assert!(page.contains("- external dependency\n"), "{page}");
    for hidden in ["kiln-plain", "Quiet work", "Quiet body."] {
        assert!(!page.contains(hidden), "{hidden} leaked\n{page}");
    }

    let root = handbook(&world, &["--tree", "kiln-plain"]);
    assert!(!root.status.success(), "{}", dump(&root));
    assert!(
        stderr(&root).contains("a --tree ticket is not marked public"),
        "{}",
        dump(&root)
    );
}

#[test]
fn a_note_does_not_name_an_unmarked_ticket() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ticket(
        "kiln",
        "kiln-root",
        "Cut the release",
        Some("Wait for kiln-private first."),
        "public",
    );
    world.ticket("kiln", "kiln-private", "Quiet work", None, "");
    let path = vissue_core::Layout::new(&world.root, "day").project_issues_path("kiln");
    let org = std::fs::read_to_string(&path).unwrap();
    let id_at = org.find("kiln-root").unwrap();
    let end_at = id_at + org[id_at..].find(":END:").unwrap() + ":END:".len();
    let logbook = "\n:LOGBOOK:\n- Note: \"Coordinate with kiln-private before release.\" [2026-01-02 Fri 09:00]\n:END:";
    let org = format!("{}{logbook}{}", &org[..end_at], &org[end_at..]);
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&path, org).unwrap();
    let page = world.ok(&["handbook", "-p", "kiln", "--tree", "kiln-root"]);
    assert!(page.contains("Cut the release"), "{page}");
    assert!(
        page.contains("Wait for external dependency first."),
        "{page}"
    );
    assert!(
        page.contains("Coordinate with external dependency before release."),
        "{page}"
    );
    assert!(!page.contains("kiln-private"), "{page}");
}

#[test]
fn a_marked_lesson_with_a_key_shaped_word_refuses_the_export() {
    if ready().is_none() {
        return;
    }
    for (n, text) in [
        concat!("Use s", "k-abc for the bench."),
        "Seed the kiln with a1b2c3d4e5f6a7b8c9d0e1f2g3 first.",
        "Set the token=abc when firing.",
        "Set password==hunter2 when firing.",
        "Set token::abc123 when firing.",
        "Note pw: hunter2 for the bench.",
        concat!("Use (s", "k-abc) for the bench."),
        concat!("Use \"s", "k-abc\" for the bench."),
        "Seed with a1b2c3d4e5f6-g7h8i9j0k1l2m3 first.",
        "Seed with a1b2c3d4e5f6.g7h8i9j0k1l2m3 first.",
    ]
    .into_iter()
    .enumerate()
    {
        let world = World::open();
        world.ok(&["remember", "--public", "-p", "kiln", text]);
        let out = world.ljos(&["handbook", "-p", "kiln"]);
        assert_eq!(out.status.code(), Some(1), "{text}: {}", dump(&out));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("secret"), "{n} {text}: {}", dump(&out));
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains(text),
            "{}",
            dump(&out)
        );
    }
}

#[test]
fn only_marked_preferences_and_rules_are_exported_and_guards_are_not() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&["prefer", "-p", "kiln", "Keep the schedule written down."]);
    world.ok(&[
        "prefer",
        "-p",
        "kiln",
        "--public",
        "Fire the small pots first.",
    ]);
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    let page = world.ok(&["handbook", "-p", "kiln"]);
    assert!(page.contains("Fire the small pots first."), "{page}");
    assert!(!page.contains("Keep the schedule written down."), "{page}");
    assert!(!page.contains("Guards"), "{page}");
    assert!(!page.contains("Protected branches"), "{page}");
}

#[test]
fn only_a_marked_workflow_is_exported_and_checked() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    let dir = world.root.join(".ljos/workflows");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        dir.join("notes.org"),
        "#+title: Notes\n\nRead /tmp/x first.\n",
    )
    .unwrap();
    let out = world.root.join("book.org");
    let exported = handbook(
        &world,
        &[
            "--workflows",
            ".ljos/workflows",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert!(exported.status.success(), "{}", dump(&exported));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        !text.contains("notes.org") && !text.contains("/tmp"),
        "{text}"
    );
    let checked = handbook(
        &world,
        &[
            "--workflows",
            ".ljos/workflows",
            "--out",
            out.to_str().unwrap(),
            "--check",
        ],
    );
    assert!(checked.status.success(), "{}", dump(&checked));

    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        dir.join("notes.org"),
        "#+public: yes\n#+title: Notes\n\nRead /tmp/x first.\n",
    )
    .unwrap();
    let marked = handbook(
        &world,
        &[
            "--workflows",
            ".ljos/workflows",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert!(marked.status.success(), "{}", dump(&marked));
    assert!(std::fs::read_to_string(&out).unwrap().contains("notes.org"));
    assert!(!std::fs::read_to_string(&out).unwrap().contains("/tmp"));
    let checked = handbook(
        &world,
        &[
            "--workflows",
            ".ljos/workflows",
            "--out",
            out.to_str().unwrap(),
            "--check",
        ],
    );
    let err = stderr(&checked);
    assert!(!checked.status.success(), "{err}");
    assert!(err.contains("a workflow file:4: unsafe text"), "{err}");
    assert!(!err.contains("/tmp"), "{err}");
}

#[test]
fn only_a_header_keyword_marks_or_titles_a_workflow() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    let dir = world.root.join(".ljos/workflows");
    let files = [
        ("plain", "public: yes\n#+title: Plain flow\n\nBody.\n"),
        ("hashes", "##+public: yes\n#+title: Hashes flow\n"),
        ("plus", "+public: yes\n#+title: Plus flow\n"),
        (
            "block",
            "#+title: Block flow\n\n#+begin_example\n#+public: yes\n#+end_example\n",
        ),
        ("body", "#+title: Body flow\n\nText.\n#+public: yes\n"),
        ("upper", "#+PUBLIC: yes\n#+title: Upper flow\n"),
        ("capital", "#+Public: Yes\n#+title: Capital flow\n"),
        ("loud", "#+public: YES\n#+title: Loud flow\n"),
        ("short", "#+public: t\n#+title: Short flow\n"),
        ("long", "#+public: true\n#+title: Long flow\n"),
        ("tight", "#+public:yes\n#+title: Tight flow\n"),
        ("tabbed", "#+public:\tyes\n#+title: Tabbed flow\n"),
        ("trailing", "#+public: yes  \n#+title: Trailing flow\n"),
        ("crlf", "#+public: yes\r\n#+title: Crlf flow\r\n"),
        ("heading", "* Part\n#+public: yes\n#+title: Heading flow\n"),
        (
            "real",
            "# a comment\n\n#+public: yes\n#+title: Real flow\n\nText.\n",
        ),
        (
            "untitled",
            "#+public: yes\n\n#+begin_example\n#+title: Block title\n#+end_example\n",
        ),
    ];
    for (name, text) in files {
        #[expect(
            clippy::disallowed_methods,
            reason = "writes a file that is not a git hook"
        )]
        std::fs::write(dir.join(format!("{name}.org")), text).unwrap();
    }
    let page = world.ok(&[
        "handbook",
        "-p",
        "kiln",
        "--workflows",
        dir.to_str().unwrap(),
    ]);
    assert!(page.contains("[[file:real.org][real.org]]"), "{page}");
    assert!(page.contains("Real flow"), "{page}");
    assert!(
        page.contains("[[file:untitled.org][untitled.org]]"),
        "{page}"
    );
    for hidden in [
        "plain", "Plain", "hashes", "Hashes", "plus", "Plus", "block", "Block", "body", "Body",
        "heading", "Heading", "upper", "Upper", "capital", "Capital", "loud", "Loud", "short",
        "Short", "long", "Long", "tight", "Tight", "tabbed", "Tabbed", "trailing", "Trailing",
        "crlf", "Crlf",
    ] {
        assert!(!page.contains(hidden), "{hidden} was published\n{page}");
    }
}

#[test]
fn a_write_that_cannot_finish_leaves_the_old_file_whole() {
    use std::os::unix::fs::PermissionsExt;
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    let dir = world.home.path().join("ro-out");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("book.org");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&out, "old book\n").unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "sets the mode of a path that is not a git hook"
    )]
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = dir.join("probe");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    let can_write_dir = std::fs::write(&probe, "x").is_ok();
    let refused = handbook(&world, &["--out", out.to_str().unwrap()]);
    #[expect(
        clippy::disallowed_methods,
        reason = "sets the mode of a path that is not a git hook"
    )]
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if can_write_dir {
        ljos_cli::say!("the directory is writable for this user, so the test was skipped");
        return;
    }
    assert!(!refused.status.success(), "{}", dump(&refused));
    let err = String::from_utf8_lossy(&refused.stderr).to_string();
    assert!(err.contains(dir.to_str().unwrap()), "{}", dump(&refused));
    assert!(err.contains("not writable"), "{}", dump(&refused));
    assert!(!err.contains("The store"), "{}", dump(&refused));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "old book\n");
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(left.len(), 1);
}

fn world_with_a_lesson() -> World {
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    world
}

#[test]
fn a_read_only_out_file_is_refused_and_left_alone() {
    use std::os::unix::fs::PermissionsExt;
    if ready().is_none() {
        return;
    }
    let world = world_with_a_lesson();
    let out = world.home.path().join("locked.org");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&out, "kept by the person\n").unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "sets the mode of a path that is not a git hook"
    )]
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o444)).unwrap();
    let refused = handbook(&world, &["--out", out.to_str().unwrap()]);
    assert_eq!(refused.status.code(), Some(1), "{}", dump(&refused));
    let err = String::from_utf8_lossy(&refused.stderr);
    assert!(err.contains(out.to_str().unwrap()), "{}", dump(&refused));
    assert!(err.contains("read-only"), "{}", dump(&refused));
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "kept by the person\n"
    );
    assert_eq!(
        std::fs::metadata(&out).unwrap().permissions().mode() & 0o777,
        0o444
    );
}

#[test]
fn an_out_symlink_is_refused_whether_it_resolves_or_dangles() {
    use std::os::unix::fs::symlink;
    if ready().is_none() {
        return;
    }
    let world = world_with_a_lesson();
    let real = world.home.path().join("real.org");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&real, "the real file\n").unwrap();
    let link = world.home.path().join("link.org");
    #[expect(
        clippy::disallowed_methods,
        reason = "links a path that is not a git hook"
    )]
    symlink(&real, &link).unwrap();
    let dangling = world.home.path().join("dangling.org");
    let nowhere = world.home.path().join("nowhere.org");
    #[expect(
        clippy::disallowed_methods,
        reason = "links a path that is not a git hook"
    )]
    symlink(&nowhere, &dangling).unwrap();
    for path in [&link, &dangling] {
        let refused = handbook(&world, &["--out", path.to_str().unwrap()]);
        assert_eq!(refused.status.code(), Some(1), "{}", dump(&refused));
        let err = String::from_utf8_lossy(&refused.stderr);
        assert!(err.contains(path.to_str().unwrap()), "{}", dump(&refused));
        assert!(err.contains("symbolic link"), "{}", dump(&refused));
        assert!(
            std::fs::symlink_metadata(path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link was replaced"
        );
    }
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "the real file\n");
    assert!(!nowhere.exists(), "a dangling link was followed");
}

#[test]
fn a_directory_out_gets_faq_org_and_check_reads_it() {
    if ready().is_none() {
        return;
    }
    let world = world_with_a_lesson();
    let dir = world.home.path().join("book");
    std::fs::create_dir_all(&dir).unwrap();
    let wrote = handbook(&world, &["--out", dir.to_str().unwrap()]);
    assert!(wrote.status.success(), "{}", dump(&wrote));
    let page = std::fs::read_to_string(dir.join("faq.org")).unwrap();
    assert!(page.contains("Publish the signed archive."), "{page}");
    let same = handbook(&world, &["--out", dir.to_str().unwrap(), "--check"]);
    assert!(same.status.success(), "{}", dump(&same));
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(dir.join("faq.org"), format!("{page}drift\n")).unwrap();
    let drift = handbook(&world, &["--out", dir.to_str().unwrap(), "--check"]);
    assert_eq!(drift.status.code(), Some(1), "{}", dump(&drift));
    assert!(
        String::from_utf8_lossy(&drift.stderr).contains("differs"),
        "{}",
        dump(&drift)
    );
    let missing = world.home.path().join("empty");
    std::fs::create_dir_all(&missing).unwrap();
    let none = handbook(&world, &["--out", missing.to_str().unwrap(), "--check"]);
    assert_eq!(none.status.code(), Some(1), "{}", dump(&none));
    assert!(!missing.join("faq.org").exists());
}

#[test]
fn a_hidden_id_in_a_lesson_a_preference_or_the_title_is_not_exported() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ticket("kiln", "kiln-q7zz", "Quiet work", None, "");
    world.ticket("kiln", "kiln-quietly", "Quiet again", None, "");
    world.ticket("kiln", "kiln-open", "Open work", None, "public");
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Do not cut before kiln-q7zz lands, or kiln-quietly.",
    ]);
    world.ok(&[
        "prefer",
        "-p",
        "kiln",
        "--public",
        "Fire after KILN%2DQ7ZZ and kiln-zzzz.",
    ]);
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Hold for kiln%\u{200b}2Dq7zz and kiln\u{ff05}2Dq7zz.",
    ]);
    for extra in [
        vec![],
        vec!["--tree", "kiln-open"],
        vec!["--title", "Before kiln-q7zz"],
    ] {
        let page = world.ok(&[&["handbook", "-p", "kiln"][..], &extra[..]].concat());
        assert!(!page.to_lowercase().contains("q7zz"), "{page}");
        assert!(!page.to_lowercase().contains("quietly"), "{page}");
        assert!(!page.contains("zzzz"), "{page}");
        assert!(
            page.contains("Do not cut before external dependency lands, or external dependency."),
            "{page}"
        );
        assert!(
            page.contains("Fire after external dependency and external dependency."),
            "{page}"
        );
        assert!(
            page.contains("Hold for external dependency and external dependency."),
            "{page}"
        );
    }
}

#[test]
fn out_dev_stdout_prints_the_page() {
    if ready().is_none() {
        return;
    }
    let world = world_with_a_lesson();
    let page = handbook(&world, &["--out", "/dev/stdout"]);
    assert!(page.status.success(), "{}", dump(&page));
    let text = String::from_utf8_lossy(&page.stdout);
    assert!(text.starts_with("#+title: Project handbook"), "{text}");
    assert!(text.contains("Publish the signed archive."), "{text}");
    assert!(!String::from_utf8_lossy(&page.stderr).contains("not writable"));
}

#[test]
fn the_export_does_not_depend_on_the_running_seat() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ticket(
        "kiln",
        "kiln-a",
        "Write the changelog",
        Some("Update the changelog and main notes."),
        "public",
    );
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Write the changelog before tagging main.",
    ]);
    world.ok(&[
        "prefer",
        "-p",
        "kiln",
        "--public",
        "Keep the main bench tidy.",
    ]);
    let mut pages = Vec::new();
    for seat in ["saddle", "changelog", "main"] {
        for ids in [false, true] {
            let mut args = vec!["handbook", "-p", "kiln", "--tree", "kiln-a"];
            if ids {
                args.push("--ids");
            }
            let out = world.ljos_with(&args, &[("LJOS_SEAT", seat)]);
            assert!(out.status.success(), "{seat}\n{}", dump(&out));
            pages.push((seat, ids, out.stdout, out.stderr));
        }
    }
    let first = String::from_utf8_lossy(&pages[0].2).into_owned();
    for needle in [
        "Keep the main bench tidy.",
        "Write the changelog before tagging main.",
        "Write the changelog",
        "Update the changelog and main notes.",
    ] {
        assert!(first.contains(needle), "{needle} missing\n{first}");
    }
    for (seat, ids, page, err) in &pages {
        let same = pages.iter().find(|row| row.1 == *ids).unwrap();
        assert_eq!(page, &same.2, "seat {seat} ids={ids} changed the export");
        assert_eq!(err, &same.3, "seat {seat} ids={ids} changed stderr");
    }
}

#[test]
fn ids_put_an_id_drawer_on_marked_items_only() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    world.ok(&["remember", "-p", "kiln", "Keep this one home."]);
    let page = world.ok(&["handbook", "-p", "kiln", "--ids"]);
    assert!(page.contains(":PROPERTIES:\n:ID: "), "{page}");
    assert_eq!(page.matches(":ID: ").count(), 1, "{page}");
    assert!(!page.contains("Keep this one home."), "{page}");
}

#[test]
fn a_real_blocker_cycle_is_reported() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ticket("kiln", "kiln-a", "First half", None, "public");
    world.ticket("kiln", "kiln-b", "Second half", None, "public");
    let path = vissue_core::Layout::new(&world.root, "day").project_issues_path("kiln");
    let org = std::fs::read_to_string(&path)
        .unwrap()
        .replace(
            ":ID:         kiln-a",
            ":ID:         kiln-a\n:BLOCKED_BY: kiln-b",
        )
        .replace(
            ":ID:         kiln-b",
            ":ID:         kiln-b\n:BLOCKED_BY: kiln-a",
        );
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(&path, org).unwrap();
    let page = world.ok(&["handbook", "-p", "kiln", "--tree", "kiln-a"]);
    assert!(page.contains("Cycle:"), "{page}");
}

fn emacs() -> Option<PathBuf> {
    let found = on_path("emacs");
    if found.is_none() {
        assert!(
            std::env::var(tools::REQUIRE_TOOLS).ok().as_deref() != Some("1"),
            "emacs is not on PATH"
        );
        ljos_cli::say!("emacs is not on PATH, so the org check was skipped");
    }
    found
}

fn lint_findings(world: &World, emacs: &Path, file: &Path) -> Vec<String> {
    let home = world.home.path().join("emacs-home");
    std::fs::create_dir_all(home.join(".emacs.d")).unwrap();
    let lisp = format!(
        "(progn (require 'org) (find-file {file:?}) (setq org-confirm-babel-evaluate nil) (dolist (r (org-lint)) (message \"LINT %s\" r)) (kill-emacs 0))"
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let lint = Command::new(emacs)
        .args(["--batch", "--eval", &lisp])
        .env("HOME", &home)
        .output()
        .expect("emacs");
    assert!(lint.status.success(), "emacs failed\n{}", dump(&lint));
    String::from_utf8_lossy(&lint.stderr)
        .lines()
        .filter(|line| line.starts_with("LINT "))
        .map(str::to_string)
        .collect()
}

#[test]
fn org_lint_and_babel_accept_a_generated_handbook() {
    if ready().is_none() {
        return;
    }
    let Some(emacs) = emacs() else {
        return;
    };
    let world = World::open();
    world.ticket(
        "kiln",
        "kiln-a",
        "Cut the release",
        Some("Publish the archive."),
        "public",
    );
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    world.ok(&[
        "prefer",
        "-p",
        "kiln",
        "--public",
        "Fire the small pots first.",
    ]);
    let dir = world.root.join(".ljos/workflows");
    #[expect(
        clippy::disallowed_methods,
        reason = "writes a file that is not a git hook"
    )]
    std::fs::write(
        dir.join("release.org"),
        "#+public: yes\n#+title: Release train\n\nPublish the archive.\n",
    )
    .unwrap();
    let out = dir.join("handbook.org");
    world.ok(&[
        "handbook",
        "-p",
        "kiln",
        "--tree",
        "kiln-a",
        "--ids",
        "--workflows",
        dir.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    let findings = lint_findings(&world, &emacs, &out);
    assert!(findings.is_empty(), "{findings:?}");

    let html = world.root.join("handbook.html");
    let home = world.home.path().join("emacs-home");
    let lisp = format!(
        "(progn (require 'org) (require 'ox-html) (org-babel-do-load-languages 'org-babel-load-languages '((emacs-lisp . t) (shell . t))) (setq org-confirm-babel-evaluate nil org-export-use-babel t org-export-babel-evaluate t) (find-file {out:?}) (org-export-to-file 'html {html:?}) (kill-emacs 0))"
    );
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let run = Command::new(&emacs)
        .args(["--batch", "--eval", &lisp])
        .current_dir(&world.root)
        .env("HOME", &home)
        .output()
        .expect("emacs");
    assert!(run.status.success(), "babel export\n{}", dump(&run));
    assert!(html.is_file(), "no html\n{}", dump(&run));
}

#[test]
fn date_refuses_year_zero_and_says_the_day_does_not_exist() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    let out = world.root.join("dated.org");
    for (date, message) in [
        ("0000-01-01", "handbook: --date: that day does not exist"),
        ("2026-02-30", "handbook: --date: that day does not exist"),
        ("2026-1-2", "handbook: --date is not YYYY-MM-DD"),
    ] {
        let refused = handbook(&world, &["--date", date, "--out", out.to_str().unwrap()]);
        assert!(!refused.status.success(), "{date} was accepted");
        assert!(
            stderr(&refused).contains(message),
            "{date}: {}",
            stderr(&refused)
        );
    }
    assert!(!out.exists(), "a refused date wrote a file");
    world.ok(&[
        "handbook",
        "-p",
        "kiln",
        "--date",
        "0001-01-01",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(std::fs::read_to_string(&out)
        .unwrap()
        .contains("#+date: 0001-01-01\n"));
}

#[test]
fn an_error_does_not_echo_a_secret_argument() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    let key_id = concat!("AK", "IA", "IOSFODNN7EXAMPLE");
    for args in [
        vec!["handbook", "-p", "kiln", "--date", "PASSWORD = hunter2"],
        vec!["handbook", "-p", "kiln", "--date", key_id],
        vec!["handbook", "-p", "kiln", "--tree", "password:hunter2"],
        vec!["handbook", "-p", "PASSWORD = hunter2"],
        vec![
            "handbook",
            "-p",
            "kiln",
            "--workflows",
            "PASSWORD = hunter2",
        ],
    ] {
        let refused = world.ljos(&args);
        assert!(!refused.status.success(), "{args:?}\n{}", dump(&refused));
        let said = dump(&refused);
        assert!(!said.contains("hunter2"), "{args:?}\n{said}");
        assert!(!said.contains(key_id), "{args:?}\n{said}");
    }
}

#[test]
fn credential_phrasing_in_a_marked_lesson_refuses_without_an_echo() {
    if ready().is_none() {
        return;
    }
    let world = World::open();
    world.ok(&[
        "remember",
        "-p",
        "kiln",
        "--public",
        "Publish the signed archive.",
    ]);
    for text in [
        "The db password is hunter.",
        concat!("Use curl -u alice", ":s3cret on the box."),
        "The pin is 4921 today.",
    ] {
        world.ok(&["remember", "-p", "kiln", "--public", text]);
        let refused = handbook(&world, &[]);
        assert!(!refused.status.success(), "{text}\n{}", dump(&refused));
        let said = dump(&refused);
        for secret in ["hunter", "s3cret", "4921"] {
            assert!(!said.contains(secret), "{text}\n{said}");
        }
        let id = world
            .atoms()
            .into_iter()
            .find(|atom| atom["text"] == text)
            .and_then(|atom| atom["id"].as_str().map(str::to_string))
            .expect("lesson id");
        world.ok(&["forget", &id]);
        world.ok(&["handbook", "-p", "kiln"]);
    }
}

#[test]
fn help_names_the_marks_and_the_refusal() {
    #[expect(
        clippy::disallowed_methods,
        reason = "starts a program outside git hook code"
    )]
    let out = Command::new(env!("CARGO_BIN_EXE_ljos"))
        .args(["handbook", "--help"])
        .env_clear()
        .output()
        .expect("spawn ljos");
    assert!(out.status.success(), "{}", dump(&out));
    let help = String::from_utf8_lossy(&out.stdout);
    let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
    for needle in [
        "marked public",
        "`--public`",
        "`#+public: yes`",
        "nothing is written",
        "Project handbook",
        "with an `:ID:` property",
    ] {
        assert!(flat.contains(needle), "{needle} missing\n{flat}");
    }
    assert!(!flat.contains("is left out"), "{flat}");
}
