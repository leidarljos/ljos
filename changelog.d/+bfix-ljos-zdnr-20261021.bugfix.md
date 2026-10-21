A real checkout that includes a `target/` subtree now keeps git
discovery for tracker locking, push gating and tracker commits, and a
dotfiles-style checkout that uses `GIT_DIR`/`GIT_WORK_TREE` keeps that
behavior as well.
