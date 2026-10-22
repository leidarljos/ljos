A fifo in place of a state log made `ljos doctor` wait in open until a writer appeared.
State-log opens now use O_NONBLOCK, keep only a regular file, and the doctor reports that log as damaged.
