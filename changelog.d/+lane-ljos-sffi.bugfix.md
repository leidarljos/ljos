ljos runs `rm -rf` and `find -delete` outside `/tmp`, and a force or mirror push, instead of refusing or asking.
Each prints one `allow\twarn\tRULE\tWARNING` line and leaves a `warn` row in `$XDG_DATA_HOME/ljos/bypass.jsonl`, from the hook, `ljos policy`, the guard shim and the gate client alike.
An older `ljos-policyd` that still refuses or asks on those rules gets the same line, unless the person names a `policyd_config`, which keeps its deny.
The ask pack leaves a forced recursive delete of a home or system tree to the guard.
A push that deletes a remote branch runs with `git-push-delete` instead of asking.
A protected branch takes a force, mirror or delete push once the sitting's consensus settles, as it takes an ordinary push, and no later.
The seat guard used to refuse a command that deletes or moves a tree holding the person's key directory, such as `rm -rf /`, `rm -rf ~` or `shutil.rmtree(os.path.expanduser('~'))`.
Such a command now runs too.
It prints `allow\twarn\tuproots-person-key\tWARNING` and leaves the same `warn` row.
The warning names the tree and the key directory.
A command runs with `changes-person-key` instead of being refused when it changes the key directory, a file in it or the directory above it, such as `rm -rf ~/.config`.
A change to the runtime directory where the hook keeps the person's latest prompt runs the same way, with `changes-kept-prompt`.
So does a write from a command or a file tool to another seat file, with `writes-seat-file`: an ljos binary, a hook or the file that registers one, the ljos config or an approval.
A `policyd_config` holding `block-on-warning true` denies such a command where policyd warns on it too.
A tool call's `ljos prefer --review --keep ID` runs with no warning, since the rule is kept as the agent's.
It runs with `prefer-keep-faked-terminal` when run under `setsid`, `script` or `env -i`, as if from the person's terminal.
That warning says the rule is kept as the agent's and stays unverified.
