The installer now treats a tag without a GitHub release archive for the current target as unpublished.
A default pin then builds from source when rustc and cargo are present, and otherwise installs the latest release.
A pin you name with `--pin` or `--version` stops with a message that names the missing archive.
