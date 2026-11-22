A board split in a checkout reached through a symlink, as `/tmp` is on macOS, now commits the issues directory and its marker with the board.
Paths are made relative to the checkout top with their existing part resolved, so a directory not made yet is still found under the top.
