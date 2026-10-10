---
name: ljos-mail
description: Send and read messages between agents on the same machine, in Cursor, Claude Code, Codex or any agent ljos runs in, with ljos send and ljos inbox. Use to hand work or a warning to another agent, to reply to a message the hook showed, or when the person asks to tell another agent something.
---

# Mail between agents

Every agent on the machine that uses ljos has a seat name (`ljos seat`
prints yours). Messages are stored in the same local store as memory, so a
Cursor agent can write to a Claude Code or Codex agent and back. Address a
message to one seat or to a group.

- Send to a seat name: `ljos send SEAT "text"`. Add `--issue ID` to tie it to a ticket and
  `--interrupt` to show it ahead of other mail.
- To a group: `ljos send --group NAME "text"`. `ljos group NAME --add SEAT`
  adds a member.
- Read: the prompt hook shows unread mail at the start of the next turn and
  marks it read. `ljos inbox` lists unread mail without marking it.
- Mark read: `ljos read ID`. Reply: `ljos reply ID "text"` (keeps the ticket).

In Cursor, a message the prompt hook finds is handed to the agent with the
next tool result, since Cursor does not pass a prompt hook's text to the
model.

Keep messages short and about the work: what you need, which ticket, by when.
