A mistyped command or flag names the closest one and the command to run, and exits 2.
An unknown ticket id, and an id that matches more than one ticket, names the next command and exits 1.
`ljos remember` with an empty text or a text past 1000 characters exits 2 and names the command to run.
A search with no hits and a recall of an empty pack name the next command.
A write with no seat, a seat the tracker does not list, and a ticket another seat already claimed each name one next command.
A half-written user-rules file names the rewrite command, and so does a damaged note queue.
