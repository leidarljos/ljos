A wgetrc could set the method and the token, and a plain wget
to GitHub was still allowed. The guard treated that file as
opaque only when the command already carried a token.

The system wgetrc, `WGETRC`, `~/.wgetrc`, and `--config` are
judged for the method and body they set. A file that cannot
be read is a write. `--no-config` leaves those files unread.
