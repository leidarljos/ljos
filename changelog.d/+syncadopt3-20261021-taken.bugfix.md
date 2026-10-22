Sync import now treats the same words on two tickets as two memories, as the pack does.
A peer's atom on another ticket was skipped as already held, and the copy of the ticket the peer dropped was never retired, because import and adopt keyed what they held and what they had taken by text alone.
The taken list now records text and issue; a list written before this change is read once through the copies this machine holds, so nothing is posted again and what the peer dropped is still retired.
