Outside a hook, `ljos search` reads the pack listing straight into the fields it uses.
It checks which user owns the pack's port once per search, not on each of its requests.
On a pack of ten thousand claims a search takes about three quarters of the time, and prints the same hits.
