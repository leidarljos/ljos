`LJOS_CLAIM_NOW`, an RFC 3339 instant, sets the time claim ages are counted to, so the `ljos list` tests no longer fail when a minute turns between the `ready` run and the `list` run they compare.
