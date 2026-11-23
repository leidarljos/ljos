A source lint scans production Rust files and `build.rs` in every workspace member for common mode-publication mistakes, non-literal command programs, and shell programs.
The lint does not see generated code, native code, or commands hidden in shell strings.
It has 45 exceptions, each matched by file, item path, and exact hit count: one lets the `ljos-hud` summon socket set the mode of its private socket directory, and 44 let a named function build or start a program.
