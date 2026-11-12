Each packsetd call sends X-Packset-Deadline as unix milliseconds, now plus that call's budget.
A 503 waits Retry-After once and sends the same request again with the same deadline.
A second 503, or a passed deadline, stops and prints the JSON error field on one line.
A sitting pack read and an MCP pack read use about one second.
