cloud guard now inspects `gh api` git-ref writes (REST and GraphQL) and
refuses cursor ref creation, ref deletion, and force updates, including
payloads read from `--input` and `@query` files.
