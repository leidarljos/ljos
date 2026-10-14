`ljos onboard --harness` now takes `auggie`, `amp`, `goose`, `openhands`, `continue`, `roo` and `kilo`.
Auggie gets a `PreToolUse` hook on `launch-process`, Amp a delegate rule for `Bash` placed first in `amp.permissions`, and goose a user plugin whose `PreToolUse` hook fails closed, so the guard blocks in all three.
OpenHands, Continue, Roo Code and Kilo Code get the server and the skill, and their guard is advisory, since none of them reads a user-level tool hook.
A YAML snippet whose top-level key the config already has, such as goose's `extensions:`, now goes under that key instead of repeating it.
