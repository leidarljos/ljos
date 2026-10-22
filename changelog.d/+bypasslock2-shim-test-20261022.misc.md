The guard shim test that checks a deny is refused now pins `LJOS_GUARD=on` and clears `LJOS_OFF`, so it passes when the suite runs with a temporary `HOME` or `LJOS_OFF` set.
