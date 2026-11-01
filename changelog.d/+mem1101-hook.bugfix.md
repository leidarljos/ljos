A session-end hook returns without reading the transcript or calling the pack.
The read and the notes run in a detached process, and a stop reads only the tail of the transcript.
A guard check, including `git commit`, no longer waits out a quiet pack's own timeout.
The hook's wall budget is 2s.
