`ljos search` reads each tracker ticket once per query, and works out once per query word whether that word is in every title.
A query of two or more words that shares one word with many titles had re-read every title for each of them, so `ljos search "store mount"` took 36 s on 0.45.4 against 6.5 s on 0.44.0 over a tracker of about 24,000 tickets.
The tickets a search prints and their order are unchanged.
