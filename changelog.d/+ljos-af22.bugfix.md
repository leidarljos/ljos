`ljos cloud up` removes the pre-push hook and the `core.hooksPath` values an earlier `cloud up` set, and leaves any other hooks path alone.
On a cloud seat the shell guard warns and still runs the line.
It warns on `--no-verify`, on a `core.hooksPath` override and on a write to a gitconfig.
