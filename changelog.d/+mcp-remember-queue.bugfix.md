The `ljos_remember` tool queues a lesson when the pack writer is down, as `ljos remember` does.
It answers with the queued line instead of an error.
The tool prints the same queued line as `ljos remember`.
That line says a later ljos call sends the lesson, from any seat.
It also warns that the pack may still refuse the lesson.
Both report one of three outcomes: stored, queued, or not queued with its cause.
The outcome comes from the remember call itself, never from the words of an error.
When the queue write fails, both say that nothing was stored and name `ljos remember` to run again.
A queued remember keeps its whole request: persona, basis, horizon, project and validity dates.
The later send uses that request unchanged.
Two requests that differ in any of those fields wait as two rows.
Replay posts each row, and when the pack already has that claim it answers with it.
ljos no longer skips a queued row when a stored lesson uses the same words.
A queued finish lesson records whether the person or an agent supplied it, and replay keeps that origin, basis and horizon.
An older queue row keeps every field it recorded and does not acquire a persona, basis or horizon.
Replay prints one compatibility line for each older row when that row leaves the queue, on whichever drain pass sends it.
That line names the fields the row could not record.
