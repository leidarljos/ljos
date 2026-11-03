A blank claim id now fails at once and says the id is empty.
Both `ljos claim ""` and the `ljos_claim` tool do this.
Before, the command line read a blank id as `ljos claim --next` and waited on the claim graph, past a minute on a large one.
Only an omitted id claims the next ready node.
