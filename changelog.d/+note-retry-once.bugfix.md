A `ljos note` whose `vissue` wrote the note and then outlived the tracker wait no longer writes the note again.
The check before a retry finds this note among the notes made since the call began, not only as the newest note, so a note another seat wrote in between does not hide it, and a note that was written is not reported as unwritten.
