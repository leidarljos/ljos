Isolate `own_home`, `person_key`, `kept_prompt`, and `common/runner` tests into temporary directories outside real `HOME` using `HoldOwnHome` and an `LD_PRELOAD` `getpwuid` helper on Linux.
