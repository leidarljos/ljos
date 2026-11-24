A seat with no absolute `HOME` refuses a `TMPDIR` that any user can write and that has no sticky bit, because another user could swap the state directory there.
The refusal names `HOME` and `XDG_STATE_HOME`.
