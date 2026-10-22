A default curlrc or wgetrc path that held a directory, a fifo, or
a symlink counted as no config, so a curl or wget to GitHub was
allowed. Only a missing path is no config now. Anything else that
is not a regular file is a write, and a fifo does not hold the
guard waiting for a writer.
