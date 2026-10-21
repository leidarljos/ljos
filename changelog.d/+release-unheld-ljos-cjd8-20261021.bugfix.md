`ljos release` of an issue the claim graph no longer holds, because a lease reclaim freed it or `ljos complete` ended it, now puts the tracker heading back to the state it had before the claim instead of failing with `release: status ready` and leaving it STARTED.
A heading another name holds is left alone, and the release still fails when the tracker has nothing of this holder's to hand back.
