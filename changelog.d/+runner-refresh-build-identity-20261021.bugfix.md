First-run runner refresh no longer keys only on the semantic version.
Skill and env refresh stamps now key on the bytes this ljos would write, so
same-version upgrades still refresh stale files ljos wrote when shipped
content changes.
