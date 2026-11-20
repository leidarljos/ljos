A sync trusted any plaintext it had kept of a sealed atom log, so a changed or stale kept file could drop a new atom or keep a removed one, and the plaintext hash written after it kept the error after the files were removed.
A log read from kept plaintexts is now used only when it agrees with `plain.sha256`, and is read again with `age -d` otherwise.
