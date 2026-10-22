The guard reads a curl or wget config before the client opens it
again, and a write to `SYSTEM_WGETRC` or to a config named with
`-K` or `--config` was not refused, so the same line could change
the file in between. Those writes are refused now, and a curl or
wget that only reads a protected config is no longer taken for a
write to it. A process already running can still change a file
between the two reads.
