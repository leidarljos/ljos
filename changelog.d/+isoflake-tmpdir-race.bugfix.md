Tests now avoid process wide TMPDIR edits by using a lock guarded temporary root override in runtime directory checks, which removes cross test NotFound races in isolated config and proc tree cases.
Note queue tests now write under a process specific target path, and the memory port integration check injects a local test port instead of expecting 127.0.0.1:8761 to be free.
