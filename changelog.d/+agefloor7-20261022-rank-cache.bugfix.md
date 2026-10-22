Whether a writer takes `rank=relevance` is remembered per auth mode.
An unauthenticated status no longer answers a later read that carries
the token, and a refusal clears only the mode that was refused.
