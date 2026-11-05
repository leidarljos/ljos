# Atoms logs

Each machine seals its own atoms of a scope into the tracker repository, one log per host and scope under `.ljos/atoms/`: `<host>.jsonl.age` for the primary scope and `<host>+<scope>.jsonl.age` for an extra one. Other machines import those logs. Until this layout, an export that found any atom changed sealed the whole log again and committed it, so every sync added a full copy of the log to tracker history. One box log of 19.9 MB was rewritten 60 times in a day, and the tracker pack reached 9.65 GiB.

A log is now a base and the chunks sealed after it. A sync seals only what changed as one small chunk, and the base stays the same blob until the chunks would come to half of it.

## Files

The base is `<stem>.jsonl.age`, where the stem is `<host>` or `<host>+<scope>`. Its plaintext starts with the header line `{"ljos_log":1,"host":H,"scope":S}`, then one atom per line, sorted by id, with `embedding` dropped. This is the single file earlier versions wrote.

The chunks are `<stem>.chunks/1.age`, `<stem>.chunks/2.age` and so on, sealed to the same recipients as the base. A chunk's first line is its header, `{"ljos_chunk":N,"host":H,"scope":S,"base":"sha256:<hex>"}`, where `N` is the chunk's number and `base` is the digest of the base's ciphertext. Every line after it is an atom line or a drop line. An atom line puts that atom under its id. A drop line is an object whose only key is `ljos_drop`, and its value is the id of an atom the chunk removes. An atom that carries `ljos_drop` beside other keys is an atom line.

The digest is `<stem>.digest`, one line of three fields: the work id of the log's plaintext, the work id of its recipients, and one digest over the ciphertext of the base and each chunk in turn. An export whose three fields match the digest in the tree writes nothing. A log that a merge or a checkout replaced under the same atoms no longer matches its third field, and is sealed again.

## Reading

Import and adopt open the base and check its header as before. They then read chunk 1, chunk 2 and on, up to the first number that is missing. A chunk applies whole or not at all. Its atom lines put their ids, the last line for an id wins, and its drop lines remove theirs.

The read ends at the first chunk that cannot be read, is past number 64, or is not sealed to this machine. It ends at one that does not open, as a file cut short does not. It ends at one whose header names another host, scope, base or number, or that carries a line that is neither an atom nor a drop. It also ends when the base's lines do not each carry a distinct id. The log then reads as the base and the chunks before that one, and import prints one line naming the chunk and why.

With no chunk applied, the view is the base's atoms as sealed, byte for byte. With chunks applied, it is their lines by id in id order, which is the plaintext a single-file seal of the same atoms carries.

## Writing

Export builds this machine's atoms of the scope as before. When the digest matches, it stops. Otherwise it seals the whole log as a new base, and removes the chunk directory, when any of these is true:

- the digest has two fields, as an ljos from before chunks wrote it;
- the recipients changed;
- the base is missing, does not open with this machine's key, or its header names another host or scope;
- the atoms, or the log as read, do not each carry a distinct id;
- the new chunk, folded as the next section says, would land on the base.

Otherwise it diffs the atoms against the log as a reader sees it, base and chunks. An atom added or changed is written whole as an atom line, and an atom that is gone is a drop line. Those lines are sealed as the chunk after the last one read, or folded into a chunk before it. Chunk files from that number on are removed first, so a chunk a reader left unread is replaced, and what it carried is in the new one. When nothing differs, nothing is sealed: chunk files past the ones read are removed and the digest is written again. The commit stages the base, the digest and the chunk directory.

`ljos finish` does not seal a log whole. It appends one chunk when this machine can open its own log: the digest has three fields, the recipients match, and the base header names this host and scope. Otherwise it prints a warning, leaves the atoms in the pack, and names `ljos sync`, which seals that log whole. Opening the log reads the files on disk. A git `index.lock` that stops the attributes stage does not decide it.

## Compaction

A new chunk folds into the chunk before it, its lines winning, while its plaintext comes to at least half of that chunk's, and so on down. The fold takes that chunk's number, and chunk files from that number on are removed. Folding into the base is a whole seal, so the log is sealed whole once its chunks would come to half the plaintext of the base. A chunk that would be number 65 folds too, as a reader opens at most 64. The rule is counted from the files in the tree and the atoms being written, so every machine that writes the same atoms over the same log folds on the same sync.

Half is the doubling of the logarithmic method. Every chunk kept is under half the one before it, so a base of B plaintext bytes carries fewer than log2(B / h) chunks, h being one chunk header's 126 or so bytes. That is 17 over a 16 MB base, inside the 64 a reader opens. A sealed line is sealed again only when a fold lands on its chunk. A fold of other atoms leaves that chunk at least half again as large, so a line is resealed at most log1.5(B / h) times before the base takes it. A sync that changes one atom of a large log commits about that atom's size, plus the chunks a fold lands on, and the base blob is written again only at a whole seal.

Before folding, an ljos sealed the log whole at the 65th chunk, or once the chunks would pass the plaintext of the base. A 16 MB log then took a fresh 16 MB blob every 65 syncs that changed an atom.

## Upgrading

The first sync after upgrading rewrites each log once. The digest an older ljos wrote has two fields, so that sync seals the log whole and writes a digest of three. After that each sync that changes an atom seals one chunk, after the last or folded into one before it, and the base is rewritten only when the chunks would come to half of it.

An ljos that reads chunks but does not fold them reads a folded log, as a folded chunk is a chunk with more lines. A log it left at 64 chunks folds on the next sync of an ljos that folds.

An ljos from before chunks still reads the base and nothing else. Until the next whole seal, it misses the atoms the chunks add, change or drop, so every machine that imports a log should run a version that reads chunks. Folding seals the base whole less often, so such a reader falls further behind.

## Conflicts

Only the machine a log names writes it. The base and the digest keep the `ljos-atoms` merge driver, which seals this machine's log whole in memory and writes only the file git hands it. Chunk files carry no merge driver. ljos's own push can stop a replay on any file of this machine's log: base, digest or chunk. It then seals the log whole from the pack and writes a digest that names no sealed file. It drops the chunk directory from the index and the work tree.

A plain git merge can still pair a base with chunks sealed over another base. Those chunks name the other base's digest, so readers leave them unread, and the next export by that machine seals over them.

## Adopting a host and removing a scope

`ljos sync --adopt HOST` reads the gone host's logs as base and chunks, and deletes every chunk file in HEAD with the base and the digest, in the one adopt commit. A chunk file that is untracked, differs from HEAD, or is not a regular file leaves the log in place, as a digest in that state does.

A scope removed from `.ljos/sync.toml` takes this machine's log, digest and chunk directory of it out of the tree on the next export.

## History already written

Chunks stop the growth from the first sync after upgrading. They do not shrink the history a tracker already has. ljos does not rewrite tracker history; that is left to whoever owns the tracker.
