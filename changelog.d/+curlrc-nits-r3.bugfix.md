The shell guard now treats an empty `$XDG_CONFIG_HOME` or `$XDG_DATA_HOME` as unset when it fills in a written path.
An empty `XDG_CONFIG_HOME` no longer adds a relative `git/config` to the protected writes.
A file named `_curlrc` stays a curl config on every platform, because a Linux build under WSL can run Windows `curl.exe`, which opens that name.
The guard's notes on curl now say that `.config/curlrc` is read only when `XDG_CONFIG_HOME` is unset, and that curl 7.65 also opened `_curlrc` on MS-DOS.
