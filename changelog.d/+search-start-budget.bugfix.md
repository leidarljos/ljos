A search spends the start wait once, not once for `/health` and again for the search, so a pack that is slow to open no longer keeps a command waiting past `LJOS_SEARCH_WARMUP_WAIT`.
