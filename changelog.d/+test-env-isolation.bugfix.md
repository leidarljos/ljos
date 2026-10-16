Tests that read or write the process environment take one lock, and each one is given a private runtime directory that is put back when the test ends.
Seen files and hook holds live in that runtime directory, so a cleared `XDG_RUNTIME_DIR` cannot land them in a shared `/tmp/ljos`.
Scratch directories and sockets those tests create are private to the test, not a path named only by the process id.
The guard a seat test takes sets aside a seat name, a hold, and a runner session the process was started with, and puts them back afterwards.
