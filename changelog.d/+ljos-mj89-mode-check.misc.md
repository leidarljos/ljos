A test reads every workspace source as tokens.
It refuses any mode change outside the publish module that could make a file executable.
Formatting, comments and an `#[expect]` around a call do not hide a mode change from it, so a file written and then made executable through a descriptor is refused.
Every other mode change goes through `ljos_cli::change_mode` or `ljos_cli::change_mode_of`.
These helpers never follow a symlink and never give a file an execute bit it lacks.
`clippy.toml` also lists `File::set_permissions` and the libc chmod calls, and `ljos_cli::program` also refuses `chmod`.
`ljos_cli::copy_file` takes the place of `std::fs::copy` and does not give the copy the mode of its source.
A data file that ljos rewrites keeps the mode of the old file without any execute bit.
