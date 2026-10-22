A depth-1 tracker checkout has no parent commits. Reading a missing
parent as "the pin was not there" refused a seat that had been pinned
before slots, and reading it as "no slots yet" treated a pin at HEAD
as standing. When the checkout is shallow or missing objects, ljos
fetches `--unshallow` from the tracker remote (and deepens if it is
still shallow). If the history is still incomplete, the refusal says
`tracker history incomplete` and names `git fetch --unshallow`.
