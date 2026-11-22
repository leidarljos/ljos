`ljos file --from` now puts back, when a row is refused, only the files the batch itself wrote, with their bytes exactly as they were.
Before, it scanned the project directory again, removed any file it had not seen at the start, and rewrote any file that differed with invalid UTF-8 replaced, so a file another writer made or changed meanwhile was lost or damaged.
