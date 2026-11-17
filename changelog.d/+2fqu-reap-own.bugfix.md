A test stops only a `packsetd` it started, and only when that start is after the test.
A writer for another `HOME` keeps running, including one that began during the test.
The stop is `SIGTERM`, and `SIGKILL` follows only after two seconds.
