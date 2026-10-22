A symlinked `~/.curlrc`, `~/.wgetrc`, or `wget --config` file was
treated as unreadable, and an unreadable default config was ignored
when the command line showed a host that is not GitHub. Curl and wget
follow the link, so a `url` or `input` in the target reached GitHub.
The guard now follows the link and judges the file when this user or
root owns it. Any config the client may load that the guard cannot
read, and a wgetrc line it cannot parse, leave the target unknown.
