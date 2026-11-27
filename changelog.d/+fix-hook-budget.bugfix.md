A guard check asks ljos-policyd for its verdict before it waits on the pack recall, so a slow pack no longer leaves the guard "unhealthy (did not answer in time)".
A guard check reads the pack listing once and shares it between the recall, the rules read and the origin record, instead of listing it twice.
A pack call that times out against a listening writer reports the busy line, not "no pack writer is answering", and no longer restarts the writer.
