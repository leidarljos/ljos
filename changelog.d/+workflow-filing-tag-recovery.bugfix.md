`ljos workflow --start` refuses a step when more than one child carries the attempt's `filing-` tag, and the refusal now names that tag and every such child.
It says to remove the tag from every one of them except the child to keep, then start again.
Closing a child did not clear the refusal, because a closed child still matches.
