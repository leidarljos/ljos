Every crate in the workspace now denies `clippy::disallowed_methods`, and `clippy.toml` lists the calls that put a file at a path or start a program, so clippy refuses a new call to one of them in any file by whatever import, binding, cast, closure or macro reaches it.
Each call that stays carries `#[expect(clippy::disallowed_methods, reason = "..")]` on the statement or match arm around it.
Code that names a git hook writes its other files with `ljos_cli::write_file`, which refuses a git hook file name and a symlink at the path, and starts programs with `ljos_cli::program`, which refuses `cp`, `mv`, `install`, `ln` and `chmod`.
The text scanners that looked for hook writes by their call syntax and for hook code outside a deny scope are removed.
A test checks that the workspace denies the lint and that every crate takes the workspace lints.
