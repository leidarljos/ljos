The first-run curlrc test prints one line and skips only the passwd home places when `unshare --user --mount --map-root-user` exits nonzero before it can bind a copy of `/etc/passwd`, as with `unshare: write failed /proc/self/uid_map: Operation not permitted` on GitHub's Ubuntu 24.04 runner, instead of panicking.
The other curlrc places still run, and the skip holds whether or not `LJOS_TEST_REQUIRE_TOOLS` is set, because a kernel that refuses user namespaces is not a tool the job can install.
A missing `unshare` binary is one, so `LJOS_TEST_REQUIRE_TOOLS=1` fails the test on that alone.
