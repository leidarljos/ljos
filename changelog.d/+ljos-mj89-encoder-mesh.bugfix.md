`ljos cloud up` publishes the encoder binary onto its installed name, so it no longer makes a file executable by path before the rename.
`ljos mesh up` publishes each Tailscale binary the same way, so it no longer changes the mode of a symlink target left at the installed name.
