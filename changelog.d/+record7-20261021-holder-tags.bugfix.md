Holder and seat matching now use one folding rule across
`holder_parts`, `same_holder`, `same_seat`, `claim_holder`,
`seat_key` and `adopts_holder`.
Legacy pid tags still fold, including all-digit tags such as `inky-10`,
while `sand-subagent-*`, `run-*` and `judge-*` names stay whole.
Seat matching now folds case consistently (`GPT-5` and `gpt-5` are one
seat; `Ridley-CI-A-555001` and `ridley-ci-a-555001` are one seat).
