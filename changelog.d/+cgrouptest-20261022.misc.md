Tests that stand in for the person no longer read the runner cgroup of the machine that runs them.
Unit tests read a login scope unless they ask for the host file, and that stand-in is left out of a release build.
On Linux a test binary other than those unit tests moves into a scope whose path names no runner before it runs, and the program it starts reads the cgroup it was started in.
