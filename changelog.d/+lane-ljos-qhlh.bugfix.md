`ljos release` and `ljos complete` fail at once on a blank id and say the id is empty.
The `ljos_release` and `ljos_complete` tools do the same, before they ask the claim graph.
Before, a blank id was hashed and the claim graph was waited on, and `release` reported success for a node nobody named.
