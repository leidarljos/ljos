A piece that starts with a NUL is no longer read as the stop word after that NUL, so `brine_\0the foo` keeps `\0the` and pairs it with the next word.
