A fine-grained token that cannot read the check rollup falls back to workflow runs and the combined commit status.
The gate message names the source it used, the rollup or the REST fallback.
A completed run must conclude success, neutral, or skipped.
A failed, cancelled, timed out, or action-required run is refused, and a queued or in-progress run is refused as pending.
The combined commit status must be success when any status exists, and zero statuses is fine.
When both are empty the merge asks, and the ask names a local evidence record when one exists for that sha.
When those REST calls fail, the refusal still says gh could not read the pull request and does not say that CI is missing.
