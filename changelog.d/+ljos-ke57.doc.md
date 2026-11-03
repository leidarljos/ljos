The contributing notes state that the identity check compares author and committer dates as seconds since the epoch.
The date test reads those seconds with git log, so it runs on macOS.
