When another tracker writer holds a lock past a sitting's wait, the line says how long the sitting waited and which limit ended the wait: `LJOS_TRACKER_LOCK_WAIT`, the claim budget or the tool budget.
It used to name the claim budget and its length even when a shorter `LJOS_TRACKER_LOCK_WAIT` had ended the wait.
