`workflow render` names the `--out` file it cannot read or write, instead of saying the store is not writable.
It refuses an `--out` that is a symlink or not a regular file, and puts the page in place through a new temporary file instead of writing into the old one.
