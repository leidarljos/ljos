`ljos consensus` no longer waits out a silent pack once for every read it makes.
The first read still uses its budget.
A later read in that run returns at once, so the waits are not added together.
