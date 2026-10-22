A pack writer whose search answers are not valid JSON, such as one
that puts a NaN in a score, no longer leaves the prompt with no
memories. ljos reads such an answer as finding nothing and adds the
stored memories that best match the prompt, as it does for a writer
that does not rank by relevance.
