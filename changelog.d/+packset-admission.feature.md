ljos builds against packset 0.17.6.
Each pack request sends `X-Packset-Deadline`.
Search and recall use a short deadline.
Remember and import use a longer one.
A pack that answers status 503 is retried after the Retry-After time, with jitter, inside that deadline.
A write that is still busy is queued, or exits 75 with `packsetd busy, retrying later`.
A busy read stops with `packsetd busy`.
Status 408 means the deadline passed before the request ran, and ljos does not send it again.
`ljos doctor` and the status line show `ok` or `busy, retrying` from `GET /stats`.
