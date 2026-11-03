ljos runs `rm -rf` and `find -delete` outside `/tmp`, and a force or mirror push, instead of refusing or asking.
Each prints one `allow\twarn\tRULE\tWARNING` line and leaves a `warn` row in `$XDG_DATA_HOME/ljos/bypass.jsonl`, from the hook, `ljos policy`, the guard shim and the gate client alike.
An older `ljos-policyd` that still refuses or asks on those rules gets the same line, and the ask pack leaves a forced recursive delete of a home or system tree to the guard.
A force or mirror push onto a protected branch still waits for the sitting's settled consensus, and a delete push still asks.
