A global `core.hooksPath` the user already set is left alone.
`ljos git-hooks install` appends a block that runs `ljos git-hook <name>` to each hook in that directory, creating a missing hook executable and keeping the existing text.
`ljos doctor` passes when those hooks run ljos, and otherwise names `ljos git-hooks install`.
`ljos git-hooks uninstall` removes only that block.
