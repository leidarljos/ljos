A GitHub host written with a capital scheme, a trailing dot,
userinfo in front of the host, or no scheme after
`--proto-default` was not recognised. The same request that
the guard used to refuse then passed, including a DELETE and
a receive-pack upload.

The host is taken from the authority after the last `@`, with
the scheme's case and one trailing dot ignored. A schemeless
URL counts when curl's default protocol or httpie supplies it.
