`ljos cloud up` no longer installs a pre-push hook or sets `core.hooksPath` at any level.
Commits and pushes on a cloud seat run only the hooks the repository and the login already name.
`ljos doctor` reports the push allowlist and no longer checks a hook.
