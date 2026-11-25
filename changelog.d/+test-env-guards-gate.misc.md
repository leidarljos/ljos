Build `test_env_guards` only for tests, so the release `ljos_cli` library no longer exports the environment mutation helpers.
The HUD copy is now private to its unit tests.
