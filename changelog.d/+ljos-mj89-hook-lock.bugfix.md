ljos takes a lock on the git hooks directory while it reads and rewrites hooks there, so two ljos commands that edit hooks at once take turns and neither loses the other's edit.
