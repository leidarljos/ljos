The org-lint tests find `emacs` on `PATH` instead of `/usr/bin/emacs`, skip with one line naming `apt-get install emacs-nox` when it is missing, and fail with that line under `LJOS_TEST_REQUIRE_TOOLS=1`.
The Linux cargo test job installs `emacs-nox` and the macOS job installs `emacs`, so all three org-lint tests run in CI.
