`ljos doctor` and `ljos_doctor` no longer print the value of a secret variable.
The `cloud age` row no longer repeats the value of `LJOS_CLOUD_AGE_SECRET` when the variable it names is unset, and the `signing key` row prints `DEEDAR_HOST_SIGNING_KEY` only when its value is a path.
The config rows show `set` for `LJOS_WAKE_LAUNCHER_KEY` and any other variable whose name ends in `_KEY`.
