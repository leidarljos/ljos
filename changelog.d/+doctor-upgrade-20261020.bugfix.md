A runner skill ljos wrote is refreshed the first time a new version runs, and `ljos doctor --fix` refreshes it too.
The sha256 of the bytes ljos wrote is kept beside the skill, and a file that no longer matches that hash is left in place.
`ljos doctor` warns about a stale skill ljos wrote and does not fail the command for it.
