A failed `install.sh` names the next step.
An unsupported system names Linux and macOS on x86_64 and aarch64, and `cargo install --locked` for the seat programs.
A missing curl, wget, tar, sha256sum or shasum names that tool.
A checksum mismatch or a truncated download removes the partial file and says to run `sh install.sh` again.
A bin directory that is not writable names `--bin-dir`.
A bin directory that is not on PATH names the line for bash, zsh and fish, and the script leaves shell profiles unchanged unless `--modify-path` is given.
A second run over the same version says that version was reinstalled, and a run over an older one names both versions.
A pinned version with no release says so and names the releases page.
