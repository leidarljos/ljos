A guard bypass log or note queue whose last row had no newline was read as healthy by `ljos doctor` for good, since that row was taken for one still being written.
Both doctor rows now name the log damaged, with "an unfinished last row", once that row has gone unchanged for five seconds; a row a writer is still appending is left alone.
