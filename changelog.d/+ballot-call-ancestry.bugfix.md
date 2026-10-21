A hook's call record now names a session only for an `ljos` that runs under the process that ran the hook.
The `PreToolUse` hook keeps its own ancestors with each shell call, and `ljos vote` takes a record only when the nearest of them still alive is one of its own ancestors and the line matches, so a record written by hand or left by a hook in another process tree binds nothing.
