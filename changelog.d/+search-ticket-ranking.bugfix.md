`ljos search` reads each tracker ticket once per query, and works out once per query word whether that word is in every title.
A query of two or more words that shares one word with many titles had re-read every title for each of them, so `ljos search "store mount"` took 36 s on 0.45.4 against 6.5 s on 0.44.0 over a tracker of about 24,000 tickets.
A long query with digits and hyphenated words, such as `ljos search "anneal 168 restack3 default-on gate"`, could run for minutes the same way; each title word is now read once however long the query is.
Reading each ticket once does not by itself change which tickets a search prints or their order.
