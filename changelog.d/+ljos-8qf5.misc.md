With `LJOS_HOOK_TIMING` set, the prompt hook prints `hook-timing rerank N claims walked`, how many stored claims its rerank stage walked.
The rerank stage finds its hits' rows by id and walks none.
The growth test checks that the count does not move when the pack grows tenfold in claims the prompt does not share.
Before, it timed the stage against a bound.
