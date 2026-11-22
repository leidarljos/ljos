Each temporary now keeps one leading dot when it replaces a dotfile such as the tracker pending list or batch record.
`git status` hides it through the `info/exclude` line ljos writes.
A tracker write also sweeps stale temporaries at the checkout root, where those two files live.
