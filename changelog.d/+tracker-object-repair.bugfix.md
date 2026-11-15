A tracker git step that fails on a bad object, a missing object, or a pack with no index names the repository and `ljos doctor` instead of git's raw error.
`ljos doctor` prints the repair steps and does not run them.
The tracker file is still written when that git step fails.
