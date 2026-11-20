A sync whose own atom log did not open on this machine removed the unread segments and sealed the rest of the log again, up to the whole log in one new segment.
It now removes nothing and seals the atoms whole into one new snapshot once, or, when this machine's key is not a recipient, writes nothing and prints a warning that names `ljos sync --key`.
