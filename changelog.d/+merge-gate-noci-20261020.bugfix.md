A merge whose head sha has no CI checks asks, and `ljos approve` records the yes.
The ask names that sha, and names a local evidence record when one exists for it.
Red or pending checks are still refused.
When gh cannot read the pull request, the refusal still names `gh auth login` and does not say that CI is missing.
A pull request with no Bugbot threads merges once its checks are green.
