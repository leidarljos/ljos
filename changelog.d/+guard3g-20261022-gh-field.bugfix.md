`gh api` treated `-fbase=main`, `-Ftitle=@file`, and `-if title=x`
as flags it did not know. No field was recorded, so the call
looked like a GET and a merge, a ref create, or a mutation
passed.

A short flag with its value attached is the same field as
`-f base=main`. Any field is a write unless the method is GET
or HEAD, or the call is a GraphQL query.
