//! Lines an EESSI seat runs are not refused by the seat's own guard.

ljos_test_boot::harness!();

#[test]
fn an_eessi_shell_is_not_refused_by_the_seat() {
    for line in [
        "source /cvmfs/software.eessi.io/versions/2023.06/init/bash",
        "module load EESSI/2023.06",
        "rg -l seat crates | xargs -I{} sh -c 'echo {}'",
        "cat <<'EOF'\nnote the hook path in prose\nEOF",
    ] {
        let rule = ljos_cli::static_guard(line);
        assert!(rule.is_none(), "{line} refused: {rule:?}");
    }
}
