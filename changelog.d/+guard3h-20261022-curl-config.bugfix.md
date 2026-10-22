A curlrc could set the method and the token, and a plain curl
to GitHub was still allowed. The guard treated that file as
opaque only when the command already carried a token.

The default curlrc, `CURL_HOME`, the XDG curlrc, and `-K` are
judged for the method and body they set, including a nested
config. A file that cannot be read is a write. `-q` or
`--disable` as the first argument leaves the default file unread.
