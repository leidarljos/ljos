`ljos record check` now fails when the tracker pin file is a symlink to a missing path, instead of reading it as no pin file and falling back to first-seen pins.
