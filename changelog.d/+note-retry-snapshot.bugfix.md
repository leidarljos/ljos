A timed-out tracker note is never sent again.
Before it sends a note, ljos reads the issue once to count its logbook entries.
After the timeout, ljos reads the logbook once and reports success only when an entry added since the note started has exactly the note text.
Otherwise the command says it could not confirm the note, exits 74 and names `ljos show ID` as the check.
Every command that writes a tracker note takes this path, `ljos note` and `ljos reopen` among them.
When its note is not confirmed, `ljos reopen` reports the reopen, says the note is unconfirmed, and exits 74.
When the tracker refuses that note, `ljos reopen` says the ticket was reopened and the note was not written, and exits 1.
In neither case does it say the reopen is on the logbook.
