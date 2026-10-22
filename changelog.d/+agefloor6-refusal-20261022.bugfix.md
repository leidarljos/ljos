A pack writer that lists `rank=relevance` and answers one search with
a 401, a 429 or a 400 about something else is no longer searched as an
older writer from then on. ljos took any 4xx to a ranked search as the
writer refusing the rank, and asked it for the fused order, which
decays with age, for the rest of the process. Only a 400 whose body
names `rank` refuses it now. Any other 4xx fails that one search: no
fused search is tried, the stored memories that best match are all it
returns, and the next search asks for the rank again.
