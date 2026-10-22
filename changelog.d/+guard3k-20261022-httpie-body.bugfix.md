HTTPie with no method sends POST when `--raw` gives a body or stdin is
not a terminal, but the guard judged `http --raw '{}' URL` and
`echo '{}' | http URL` as GETs and let them reach GitHub. A `--raw`
in any unique prefix, a pipe into `http`, and a `<` redirect,
here-document, or here-string now make the request a write unless
`-I` or `--ignore-stdin` is given. A request item is a body only when
its first separator is `=`, `:=`, `=@`, `:=@`, or `@`, so a `==` query
or a `:` header no longer is.
