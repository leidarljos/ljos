`ljos sync` reads a log only under the name export writes for its host
and scope. `HOST+PRIMARY.jsonl.age` read as the same log as
`HOST.jsonl.age`, so an older copy committed under that name retired the
host's current atoms and shrank what this machine had taken from it.
