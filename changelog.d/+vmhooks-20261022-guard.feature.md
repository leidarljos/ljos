The cloud environment installs ljos from `develop` with `install.sh --cloud`, which onboards the cursor harness before `cloud up`.
The committed hooks file runs `ljos hook` and fails closed, so a command is denied when ljos is not installed yet.
Any repository can commit the same two files. The daemon reads them only as it starts, and it refuses a hooks file reached through a symlink.
