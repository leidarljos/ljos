`ljos mesh pull` stats each lane export and skips a file whose size and modification time are unchanged, instead of reading it again.
A file whose time changed but whose bytes did not is hashed and not imported again.
Each export is stored before the next is read, so a pull that stops keeps the finished exports and the next pull continues.
On a terminal, one line per export shows the count.
The seed is rewritten only when its bytes change.
