`policyd_config = "PATH"` in the user's or the seat's `ljos.toml` names a policyd config.
The hook, `ljos policy`, `ljos doctor` and the guard shims then ask `ljos-policyd check --config PATH`.
With `block-on-warning true` in that config, a warned command becomes policyd's deny.
A check that fails open still allows.
It is off by default: with nothing named, `check` is asked as before and every warning stays a label.
