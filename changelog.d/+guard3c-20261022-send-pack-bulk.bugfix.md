cloud guard now evaluates `git send-pack` bulk flags (`--all`,
`--mirror`, `--tags`) against local refs so hidden `cursor/*` branch or
tag updates are refused the same way as guarded `git push` bulk forms.
