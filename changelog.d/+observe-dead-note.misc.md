The unreached hook path that wrote a second commit note on `PostToolUse`, with `commit` in place of the sha, is gone.
git's post-commit hook stays the one writer of the commit note, and a test checks that a runner's `PostToolUse` or `SessionEnd` call writes none.
