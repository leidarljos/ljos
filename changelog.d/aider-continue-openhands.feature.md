`ljos onboard --harness aider` writes the skill as `~/.aider/ljos-conventions.md` and adds it to the `read:` list in `~/.aider.conf.yml`; the guard there is advisory.
`ljos onboard --harness continue` also writes Claude-shaped hooks to `~/.continue/settings.json`, which cn's source reads but cn 1.5.47 does not yet fire.
`ljos_file` names its type tag `issue_kind` in its schema and still accepts `kind`, since OpenHands refused to load the server over a field named `kind`.
