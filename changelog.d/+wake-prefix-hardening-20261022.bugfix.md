Prefix wakes now fire only for canonical run-seat slugs and skip the bare
prefix key itself.
Webhook envelopes quote the `read` helper safely and include `via` when a
prefix matched.
Wake output now names the launcher seat and the matched prefix, and
`wake --clear` explains when a seat is covered only by a prefix.
