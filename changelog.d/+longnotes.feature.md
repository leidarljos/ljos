`ljos note ID --file PATH` keeps a land summary, a report, a gate verdict or a log as a long note: a plain org page at `notes/LEAF/SLUG.org` beside the ticket's issue file, linked from one stub line of under 200 bytes on the logbook.
The page and the stub land in one tracker commit, and the stub is inserted into the logbook without rewriting the rest of the issue file.
Markdown is converted to org with pandoc, or with a small converter built into ljos when pandoc is missing or `LJOS_PANDOC` is `off`, and a log goes inside `#+begin_example`.
A page is plain org that can be opened and edited in place; its path is its name, and a byte-identical body on the same ticket is noted once.
A failed write removes its temporary and leaves no page and no stub.
A body past 256 KiB is kept whole with a warning, and past 1 MiB the page keeps the first and last 128 KiB while the full body goes to the local deedar store and is cited on the ticket; `--refuse-trim` refuses instead.
A binary or encoded body exits 1, writes nothing, and names `deedar create file`.
A short `ljos note ID TEXT` past 4 KiB, or with a line past 2 KiB, is kept whole as a long note in the same way and says so, rather than being refused or dropped, and `ljos_note` does the same.
`ljos notes ID` lists a ticket's long notes, `ljos show ID --long [NOTE]` prints one, and `ljos notes import DIR --ticket ID [--dry-run]` notes a kit's `.md`, `.txt` and `.log` files once each in one commit.
`ljos search --long` searches the long notes through a gitignored local index that rereads a page whose size or modification time changed, and a plain `ljos search` never opens a note page or the index.
The MCP server gains `ljos_notes` and `ljos_show_long`, and `ljos_note` takes `file`, `title` and `kind`.
