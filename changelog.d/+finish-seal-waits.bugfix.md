`ljos finish` no longer seals the ticket log while that ticket's lesson waits in the note queue.
It queues one seal per tracker instead, and each runs only after the lesson has left the queue, so the log is not sealed from older atoms without the lesson.
