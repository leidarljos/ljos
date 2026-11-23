Read the passwd home with `dscl` where `getent` is missing, as on macOS, in the guard shim and `scripts/install.sh`.
When no passwd home can be read, a `TMPDIR` is no longer a temp root in the shim, `scripts/install.sh` or ljos, so the guard and memory stay on when a `TMPDIR` contains the real home.
