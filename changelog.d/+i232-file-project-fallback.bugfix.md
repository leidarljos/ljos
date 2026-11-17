`ljos file TITLE` no longer invents a project from the checkout directory name.
When no parent and no `-p` are present, it uses `LJOS_PROJECT` when set, or exits 1 with an error that names `-p NAME`.
