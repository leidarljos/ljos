Sync import no longer reads a tracker log whose file name does not give a host name and a scope name.
A removed scope's log was read by any suffix, so `peer+...jsonl.age` named scope `..` and its taken list was written outside the tracker's own state directory; such a log is now skipped with a line naming the part that was refused.
