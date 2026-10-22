A sync import that reads the taken list shared by scope name and host now removes it once the tracker's own list is written.
Left in place, it was read again whenever the tracker had no list of its own, such as after an adopt; a failed write keeps it for the next import.
