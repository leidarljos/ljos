`prepare_tracker` now ignores exported `GIT_DIR` and `GIT_WORK_TREE` when it prepares another path, so it initializes and configures that tracker without rewriting an unrelated dotfiles repository.
