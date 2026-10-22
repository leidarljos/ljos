`ljos send` and `ljos reply` in push mode push the sealed log from the detached wake before the wake fires, so a seat on another machine does not read an empty inbox.
The send does not wait on the push.
A push that fails is a line in the wake log, and the wake still runs.
