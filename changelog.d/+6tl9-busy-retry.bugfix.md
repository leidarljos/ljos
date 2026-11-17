`ljos search` and `ljos recall` ask a busy pack again for `LJOS_SEARCH_BUSY_WAIT` seconds (2 when unset, `0` does not wait), and they follow `Retry-After` or a JSON `retry_ms`.
They rank by words when the pack stays busy, and one line says memory search was busy.
`ljos remember` uses that same wait. When the pack stays busy, nothing was stored, the line says to run `ljos remember` again, and the command exits 1.
A busy answer is status `429` or `503`, or a JSON `busy` or `code` field. Words in the body are not that signal.
