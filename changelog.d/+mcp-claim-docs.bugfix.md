`ljos finish --close` now retries a pending claim only after the person, decision and workflow checks pass, so a refused close writes nothing.
The warning on a raw `vissue cancel`, or an update to `CANCELLED`, under a runner now names `ljos finish ID --status cancelled --close`, the finish that cancels the ticket.
