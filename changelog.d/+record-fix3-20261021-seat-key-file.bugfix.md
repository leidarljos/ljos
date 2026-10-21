Each seat and record voter now gets its own ballot key file: `a/b_c` and `a_b/c` used to share `seats/a_b_c/ballot.key` and sign with one key.
Key directory names escape every byte other than ASCII letters, digits, `-` and `.` as `%HH`.
A key at the old folded name, such as a judge's `judge_<model>`, moves to the new name on first use, so its tracker pin still matches.
