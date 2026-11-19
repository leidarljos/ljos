`ljos remember --json` now prints one claim object for a single claim, including a duplicate or queued claim.
When text splits into more than one claim, it prints one document with a `claims` array and `stored` and `queued` counts instead of a stream of claim objects.
That document still prints when every split claim is a duplicate or when the split later fails.
Plain split output reports stored and queued counts once.
