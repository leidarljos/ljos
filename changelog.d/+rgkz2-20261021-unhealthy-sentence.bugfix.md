The relay no longer denies a tool call because its answer contains
the unhealthy-guard sentence. That match missed the sentence once a
push warning or a preference was joined to it. A real fault is
already a deny from the check. An allow that carries the sentence
stays that allow.
