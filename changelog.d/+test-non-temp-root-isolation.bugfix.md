Remove the real passwd home fallback from `proc_tree::test_non_temp_root` and `tests/common/runner.rs` so non-temp test roots only use `target` or an isolated non-temp `HOME`.
