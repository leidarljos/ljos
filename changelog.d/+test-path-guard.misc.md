Clippy rejects direct process-environment writes outside the test guard module and documented production call sites.
The test guards serialize each write and restore the environment before releasing their lock.
