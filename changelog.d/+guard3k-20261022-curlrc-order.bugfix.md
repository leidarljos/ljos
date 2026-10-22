The guard read `$XDG_CONFIG_HOME/curlrc`, which curl never loads, and
skipped the files curl 8 does load: `$XDG_CONFIG_HOME/.curlrc`, and
with `XDG_CONFIG_HOME` unset, `~/.config/curlrc`. A `request = DELETE`
there left a curl to GitHub allowed, and a write to `~/.config/curlrc`
was not refused. The guard now walks curl 8's order, lets a missing
`$CURL_HOME/.curlrc` fall through as curl does, reads no Windows file
on Linux, and refuses a write to any file in that order.
