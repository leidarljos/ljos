Test seat homes sat under the checkout's `crates/target`, so a git call in a scratch repository whose `.git` was missing walked up and could write `core.hooksPath`, `user.name` and `user.email` into the ljos checkout's `.git/config`.
Seat homes now go under `~/.cache/ljos-test-seats`, and the checkout is used only when `HOME` is a temp root or too long for a socket path.
Every git call in the tests goes through `ljos_test_boot::git`, which sets `GIT_CEILING_DIRECTORIES` to the scratch roots, so git stops at them and finds no repository.
Each scratch directory gets a name no other test can take, and no test removes another test's directory before it is done.
