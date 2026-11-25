Doctor's crates.io lookup passes `-q` to curl first, so a curlrc cannot add a URL the arguments do not name.
The first-run stand-in refuses a call that does not start with `-q` or `--disable`, which is when curl reads a config file, instead of trusting a list of paths.
