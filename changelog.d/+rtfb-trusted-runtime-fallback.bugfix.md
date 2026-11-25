With `XDG_RUNTIME_DIR` unset, ljos keeps runtime records under the temporary directory only when every directory to it passes the check the temporary state root gets: owned by root or the user, and sticky when another user can write it.
`ljos-UID` and the home's directory under it are made and opened through descriptors without following a symlink, must be the user's and closed to other writers, and are checked again by path after they are made.
When the temporary directory fails the check, the records go under `~/.cache/ljos/run`, and with no absolute home ljos warns and keeps none.
