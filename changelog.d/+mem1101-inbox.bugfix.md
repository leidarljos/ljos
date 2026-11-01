`ljos inbox` listed every atom, so a slow pack held the command for the pack timeout while `/health` still answered.
Inbox now reads kinds `message`, `receipt` and `group`.
Mail, inbox, recall and the hook use a short connect timeout and a short total timeout (`LJOS_PACK_CONNECT_MS`, `LJOS_PACK_TIMEOUT_MS`, `LJOS_HOOK_PACK_TIMEOUT_MS`).
A timeout prints `pack busy, try again` and exits 75.
A hook prints nothing and exits 0.
The call is not tried again. A silent writer does not restart the token proof, so one command spends its budget once.
`ljos inbox` writes a receipt for each unread message it shows.
`ljos inbox --all` shows mail already read, and `ljos reply` still answers it.
