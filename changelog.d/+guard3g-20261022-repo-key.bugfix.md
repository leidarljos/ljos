The push allowlist keyed a repository by dropping everything before
the first `@`. A path that contained `@` then matched the seat, and
the push was allowed. The key now keeps that path: userinfo is only
the last `@` in the part before the first slash.
