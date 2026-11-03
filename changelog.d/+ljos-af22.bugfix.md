`ljos cloud up` removes the pre-push hook and the `core.hooksPath` values an earlier `cloud up` set, and leaves any other hooks path alone.
On a cloud seat the shell guard warns and still runs the line.
It also warns on `--no-verify`, a `core.hooksPath` override and any gitconfig write.
It warns on a write to the push allowlist and on a write to a curl or wget config.
