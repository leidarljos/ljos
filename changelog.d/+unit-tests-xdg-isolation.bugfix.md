Isolate `HOME` and XDG directories to a scratch non-`/tmp` directory in `test_env_lock` and take `test_env_lock` in every unit test that resolves state or config paths.
