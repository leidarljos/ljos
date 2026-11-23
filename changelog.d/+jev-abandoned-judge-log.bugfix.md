Resolve `jev` state and cache paths before spawning prompt judges and run `ljos hook` judges in a detached `--judge-worker` session so late replies within `budget_ms` persist after the hook exits.
