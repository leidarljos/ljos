`curl -G` turned every body into a read, so `-G -T` and `-G -F`
still uploaded and passed. A curlrc or wgetrc could set the
method, and writing those files was allowed. `wget -e
method=DELETE` was a read.

An upload stays a write beside `-G`. A client config is a write
when the command carries a token, because the file can hide
the method. Writing the config file is refused.
