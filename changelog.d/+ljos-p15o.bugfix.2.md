The check for git calls without `GIT_CEILING_DIRECTORIES` now reads library sources under `src/` as well as tests.
It sees a git program held in a variable or written as a path such as `/bin/git`, and an `env_clear` in a later statement that drops the ceiling.
