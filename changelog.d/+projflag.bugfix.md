`ljos search` and `ljos remember` take `-p` and `--project`.
`ljos search -p NAME` keeps that project's tickets and lessons, and lessons that name no project.
`ljos remember -p NAME` records that project on the lesson.
An unknown project name exits 1 and names the projects that exist.
A verb that does not take `-p` names the verbs that do.
`ljos sitting` takes `-p` with `--plan`, and without `--plan` it names those verbs.
`ljos_search` and `ljos_remember` take the same `project` argument.
