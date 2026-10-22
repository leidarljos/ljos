Curl and wget percent-decode a URL host before they connect, so a
write to `https://api%2egithub%2ecom/user` reached GitHub while the
guard read a name that is not GitHub's. The guard now decodes the
host until nothing changes before the GitHub check, on the command
line, after `--url`, and in a curlrc `url` line. An encoded `@` or
`:` splits the userinfo or the port, and a trailing dot is the same
host.
