Tracker notes within 60 seconds of the checkout's last tracker commit now share the next commit, instead of making one commit each.
A held note stays on disk and on the pending list until a commit carries it.
The first note after the window commits it, and so do any other tracker write, a push, the end of the session and the exit of the process that wrote the note.
Its body names every held note.
Only a plain note waits.
Claims, ballots, finishes, filings and notes on options commit at once.
A push commits every held note, also one missing from the pending list, and a commit forgets only the held notes it carried.
`LJOS_TRACKER_BATCH_SECS` sets the window, and `0` commits every write.
