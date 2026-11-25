`workflow define` writes the deed temp file with `O_EXCL` in the temp directory.
A symlink planted at that path is not followed, so the link target stays unchanged.
