---
name: ljos-mail
description: Send and read messages between agents on the same machine, in the editor harness, the terminal harness, the command harness or any agent ljos runs in, with ljos send and ljos inbox. Use to hand work or a warning to another agent, to reply to a message the hook showed, or when the person asks to tell another agent something.
---

# Mail between agents

Every agent on the machine that uses ljos has a seat name (`ljos seat`
prints yours). Messages are stored in the same local store as memory, so an
editor harness can write to a terminal harness or a command harness and back. Address a
message to one seat or to a group.

- Send to a seat name: `ljos send SEAT "text"`. Add `--issue ID` to tie it to a ticket and
  `--interrupt` to show it ahead of other mail.
- To a group: `ljos send --group NAME "text"`. `ljos group NAME --add SEAT`
  adds a member.
- Read: the prompt hook shows unread mail at the start of the next turn and
  marks it read. `ljos inbox` lists unread mail and marks what it shows.
  `ljos inbox --all` shows mail already read.
- Mark read: `ljos read ID`. Reply: `ljos reply ID "text"` (keeps the ticket,
  and still answers a message after it is read).
- A message carries up to 16,000 characters. A longer report goes in a
  file: `deedar create file --path FILE --agent SEAT`, then send the
  accession it prints.

In the editor harness, a message the prompt hook finds is handed to the agent with the
next tool result, since the editor harness does not pass a prompt hook's text to the
model.

An agent that is idle sees mail only when something starts its next turn.
`ljos wake --command CMD` sets a command that runs when mail reaches your seat, such as a command that resumes the session, and `ljos wake --webhook URL --key-file FILE` wakes a seat through its webhook.
A wake name ending in `-` matches every longer seat that starts with it, so mail to a run reaches the launcher for that prefix.
`ljos send` starts the recipient's wake for you.
In push mode (`LJOS_TRACKER_GIT=push`) the wake pushes the sealed log first, so a seat on another machine can read the letter. The send does not wait. A failed push is written to the wake log and the wake still runs.
The envelope carries that commit: `LJOS_WAKE_COMMIT` for a command, and `commit` in the webhook JSON. It is left out when no log commit was made.
`ljos inbox --sync [COMMIT]` fetches until the remote has that commit, for up to 30 seconds (`LJOS_INBOX_SYNC_WAIT`), or else takes the remote head, imports, and lists. With no remote it only lists.
A pack call from mail, inbox, recall or a hook has a short connect timeout (`LJOS_PACK_CONNECT_MS`, default 300) and a short total timeout (`LJOS_PACK_TIMEOUT_MS`, default 4000, at most 5000; `LJOS_HOOK_PACK_TIMEOUT_MS` in a hook, default 1500, at most 2000). On a timeout the command prints `pack busy, try again` and exits 75. A hook prints nothing and exits 0. The call is not tried again.
An agent still working gets interrupt mail when it is about to stop, so use `--interrupt` when the work cannot wait.
A seat name ljos has never seen is refused with the closest one it knows.

Keep messages short and about the work: what you need, which ticket, by when.
