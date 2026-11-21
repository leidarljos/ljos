Tracker notes within 60 seconds of the checkout's last tracker commit now share the next commit, instead of making one commit each.
A held note stays on disk and on the pending list, and the first note after the window, any other tracker write, a push or the end of the session commits it, naming every held note in the body.
A claim, a ballot or a finish never waits.
`LJOS_TRACKER_BATCH_SECS` sets the window, and `0` commits every write.
