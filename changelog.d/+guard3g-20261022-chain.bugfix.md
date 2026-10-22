`ljos cloud up` replaced an existing `core.hooksPath`. The
identity hook booked there stopped running, and the guard then
refused putting it back. The same rule refused a local
hooksPath on a throwaway repository, so the bookkeeping fixture
could not run.

When a hooks directory is already booked, `cloud up` chains a
pre-push that runs the previous hook and then the seat hook.
The booked path stays. A throwaway repository can still set
its own.
