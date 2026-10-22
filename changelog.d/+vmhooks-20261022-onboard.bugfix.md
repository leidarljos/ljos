`install.sh --cloud` runs `ljos onboard --harness cursor` before `ljos cloud up`.
It used to leave onboard off, so a cloud VM never wrote the guard's hooks or the MCP entry until a later manual onboard.
When the binaries are already installed, those two commands are all that run.
