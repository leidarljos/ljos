Tracker verbs write the org file without waiting on packsetd or a quiet claim graph.
A sitting whose claim graph does not answer within about two seconds still stamps the tracker and says so in one line.
`ljos note` and `ljos finish` keep that write when the pack is down, and queue the lesson instead of failing.
`ljos remember` queues a lesson and exits 0 when the pack is busy.
`ljos search` falls back to word-ranked local rows when the pack is down or the encoder is loading.
A tracker commit waits about two seconds, then defers, and the next writer commits what was left.
`ljos file` prints the new id alone on the first line of stdout, and `--id-only` prints only that id.
`ljos start` is `ljos sitting`.
