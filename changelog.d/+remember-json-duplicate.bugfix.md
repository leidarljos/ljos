`ljos remember --json` now prints one claim object for a single claim, including a duplicate or queued claim.
When text splits into several claims, it prints one document with a `claims` array and `stored` and `queued` counts instead of a stream of claim objects.
An all-duplicate split and a split that later fails still print that document, and plain split output reports stored and queued counts once.
