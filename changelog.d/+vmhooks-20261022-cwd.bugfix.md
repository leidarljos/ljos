A tracker directory inside the working repository was published as that repository, so a failed clone committed the merge attributes into the product.
`cloud up` now keeps the local tracker outside that repository and does not write or commit there.
With the remote unset, the working repository is not used as the tracker either.
