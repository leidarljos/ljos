A sync whose own atom log did not open on this machine removed the unread segments and sealed the rest of the log again, up to the whole log in one new segment.
It now writes nothing for that log and prints a warning that names `ljos sync --key`.
