`ljos inbox --sync` fetches the tracker until the named commit is present, imports the sealed logs, and then lists.
A commit that is not on the remote falls back to the remote head.
With no remote the flag only lists.
