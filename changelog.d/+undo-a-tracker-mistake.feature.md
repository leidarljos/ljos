`ljos reopen ID` now puts any finished or cancelled ticket back to work.
It used to undo only a close this program wrote, from the seat that wrote it, so a hand close or another seat's close stood.
The ticket goes back to the state the close took it from, and the reopen is a dated note beside the close.
`ljos withdraw ID` takes back this seat's ballot on a ticket, as `ljos vote ID --withdraw` does.
`ljos withdraw ID --note TEXT` takes back a note the seat wrote.
The note stays on the logbook with the withdrawal under it, so a reader sees both.
A note ljos keeps for itself, such as a ballot signature, is not a note a seat takes back this way.
