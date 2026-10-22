A pack writer that lists `rank=relevance` in its status and then
refuses it is searched as an older writer. The hook used to read the
refusal as a writer that is not answering, gave up when `PACKSET_URL`
was set, and fell back to the fused search with no stored memories
added, so an old lesson behind fresh lookalikes never reached the
prompt. ljos now asks again without the rank, adds the stored
memories that best match, and does not send the rank to that writer
in that workspace until it reads the writer's status again.
