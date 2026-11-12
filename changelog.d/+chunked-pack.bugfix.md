A packsetd that streams an atom listing with chunked transfer encoding is read as the JSON.
Mail, the lean listing, search and status use that reader, so `ljos inbox` and `ljos finish` no longer stop on the chunk sizes.
