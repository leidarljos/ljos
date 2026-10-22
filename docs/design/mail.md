# Mail

A seat may read the owner's mail and may draft mail in the owner's name. It
must never send on its own. Reading goes through two opt-in MCP tools over a
local notmuch database. Sending goes through a separate Unix user,
`mailgate`, that alone holds the mail password and the send path, and that
sends one exact message only after a ballot, the content checks and the
owner's own yes. Everything specific to one machine (addresses, hosts,
password entries, wrapper paths, the deny file and route rules) lives in
config, never in this repository.

What ljos implements today is the reader (Config and Reading below), the
content checks, and the exporter that writes an approval file. mailgate,
its socket, the outbox, the ballot on `send H`, the approval card and
the audit receipt are not in ljos yet. Where this document describes
them, it states what they must do, not code that exists.

## Config

The `[mail]` table is optional. Without it the mail tools refuse and
nothing else changes. It names the From a draft may carry, the deny list
and the mailbox, so only the user may set it: it is read from the user's
own `~/.config/ljos/ljos.toml` and then `~/.config/ljos/mail.toml`
(under `XDG_CONFIG_HOME` when that is set), the second overriding the
keys it sets. Each must be owned by the user running ljos and not be
group or world writable. Any other layer of ljos config (`./ljos.toml`,
`./vissue.toml`, `VISSUE_CONFIG`, `LJOS_CONFIG` and
`~/.config/vissue/config.toml`) that carries a `[mail]` table, even an
empty one, is an error, so a seat that can write its working directory
cannot swap the identity or the deny list. A file that cannot be read
or does not parse, or a `[mail]` key ljos does not know, is an error
rather than a silent default. `identity` without `deny_file` is an
error too, and the checks refuse every draft when either is unset.

```toml
[mail]
# NOTMUCH_CONFIG for every read. Required: without it the reader refuses
# and never opens notmuch's default database, so an empty [mail] table
# turns nothing on.
notmuch_config = "/srv/mail/notmuch-config"
# The notmuch binary. Unset is `notmuch` on PATH.
notmuch = "/usr/bin/notmuch"
# The only From a draft may carry.
identity = "owner@example.test"
# Addresses, domains, handles, terms and pairs a draft may not carry.
deny_file = "/etc/ljos/mailgate/deny.txt"
# Attribution phrases refused on top of the built-in ones.
attribution = ["drafted by the assistant"]
```

## Reading

`ljos_mail_search` and `ljos_mail_show` are not in the core tool set, so a
session that does not ask for them pays nothing for them. Add them with
`LJOS_MCP_TOOLS=core,mail_search,mail_show`; a call to either tool
without that is refused. A cloud seat gets a refusal: it has no
database, and personal mail stays on the machine that holds it. Any
file `ljos cloud up` leaves in the ljos config dir (the cloud guard
rules or the push allowlist), whatever it holds, counts as a cloud seat
here.

- `ljos_mail_search {query, limit}` runs notmuch with a fixed argv. The
  query is one argv element after `--`, so it cannot become an option. The
  limit is at most 50. It returns thread id, date, tags and counts, with
  subject and authors wrapped as untrusted. Mail tagged `spam` or `deleted`
  is left out unless the query names that tag.
- `ljos_mail_show {thread}` takes a 16-digit thread id. Messages in the
  thread tagged `spam` or `deleted` are left out. The thread is refused
  when a message file notmuch names is a symlink or resolves outside
  notmuch's mail root (`database.mail_root`, else `database.path`).
  Text parts are wrapped as untrusted. HTML is reduced to the text a mail
  client would show. Attribute values are entity-decoded before use, and
  CSS has its comments removed and its escapes decoded. As in HTML, `/>`
  closes only a void element or one inside `<svg>` or `<math>`. Text the
  reducer can tell is concealed is dropped: inside an element with the
  `hidden` attribute, a class or id rule in a `<style>` block that
  conceals, or a style that sets `display:none`, `visibility:hidden` or
  `collapse`, `content-visibility:hidden`, `mso-hide:all`, a width or
  height of 1px or less, an offset or indent that is negative or 1000px
  or more, `clip`, `clip-path` or any transform; under an opacity (a
  number or a percentage, multiplied down the tree) of 0.05 or less or a
  font size (resolved down the tree) of 1px or less; or drawn in a color
  whose alpha is 0.05 or less, or within 16 of its background in every
  channel (black on white when nothing sets them). A value of one of
  those properties the reducer cannot read, or any `var()`, counts as
  concealing. Any `<style>` block or concealment sets `hidden_content:
  true` on the message. Rules inside `@media`, `@supports` and other
  block at-rules are read as if they apply; an `@` inside a CSS string
  or comment is not an at-rule. When the HTML does what the reducer
  cannot follow (`@import`, a concealing rule on an element or other
  selector, a rule nested in a style rule or blocks nested more than 8
  deep, a stylesheet color within 16 of a background set
  anywhere in the part, or of white when its rule sets no background, a
  stylesheet background within 16 of black when its rule sets no color,
  a linked stylesheet, or a `style`, `class`, `id` or `hidden` attribute
  holding a reference the reducer cannot decode), the part's text is
  replaced by a note that it was withheld. Links are shown as text with
  their target, never followed.
  Attachments are listed by name, type, size and sha256, with no content
  and no path. Hashing reads at most 64 MiB of one attachment; past that
  notmuch is stopped, `sha256` is null and `size` is a lower bound.

A message whose MIME parts nest more than 16 deep is refused, and replies
nested more than 64 deep in one thread are left out with `truncated`
set. Every notmuch run has a 30 s deadline. At the deadline notmuch and
its process group are killed and the tool refuses, so a hung database or
a slow part cannot stall the MCP server.

Untrusted text sits between a start and an end marker that carry a fresh
random nonce, so a message cannot close its own wrapping. Mail text is
data. A message that says the owner approves a send, or tells the reader
to call a tool, is a message, not an approval or an instruction.

## mailgate

`mailgate` is a system user with no login shell and no sudo. Its home,
password store and mail client configs are its own, mode 0700 and 0600.
Seats reach it only through a socket, `/run/mailgate/sock`, mode 0660 and
group `mailclient`. The daemon checks `SO_PEERCRED` and takes three
operations: `sync`, `status`, and `send` with an approval file and a card
reference. No operation takes a command, a path or an environment
variable. It runs with a fixed environment and only programs a seat
cannot write. The Maildir and database are owned by `mailgate` and
readable by group `mailread`, so seats can read but never write them.

Sync runs as two checked steps, the IMAP fetch and then `notmuch new`,
and reports each exit code, so a failed fetch is not hidden behind a
successful index.

If a seat can become root, or run commands as `mailgate`, the separation
protects nothing. Removing passwordless sudo from the seats' user is a
precondition, not an option.

## The send gate

1. A seat drafts. The outbox renders the message at queue time, with
   `Message-ID` and `Date` set, in one canonical form: RFC 5322 header
   fields and body with LF line endings, no carriage return anywhere.
   mailgate writes exactly those bytes to `sendmail -i -t`, and the MTA
   turns each LF into CRLF on the wire. `-i` keeps a line holding only
   `.` as text, and `-t` takes the recipients from `To` and `Cc`. The
   exporter refuses a message without exactly one `Date` that parses as
   an RFC 5322 date-time and exactly one `Message-ID` in msg-id syntax
   (`<left@right>`, dot-atom text or a `[...]` literal), since an MTA
   would add or replace them. It refuses `Bcc` and `Resent-Bcc`, which
   `-t` strips, so the sent bytes would no longer be the hashed ones.
2. Export. The exporter runs the content checks below first and writes
   no file for a draft they refuse. The approval file is plain text: a
   statement block, a blank line, then the message bytes.

   ```
   ljos-outbox v1
   draft <outbox draft id>
   reachbook-hash <the outbox's sha256 of the message, hex>
   expires <RFC 3339, at most 24 h out>
   nonce <16 random bytes, hex>

   <the message, LF line endings>
   ```

   `H` is the sha256 of the message bytes after the statement block,
   which are exactly the bytes mailgate writes to `sendmail` stdin.
   mailgate hashes the bytes it is about to write and refuses on a
   mismatch with `H`. The expiry and nonce are not in `H`; mailgate
   checks them and spends the nonce. `reachbook-hash` must equal `H`,
   so the outbox and the file name the same bytes; any other value is
   refused. The exporter refuses control characters other than tab and
   newline, and the bidirectional overrides, anywhere in the message, so
   a terminal cannot hide text from the person reading it.
3. Consensus. The draft's send issue carries the option `send H`. It
   passes with ballots from at least two seats of at least two model
   families, each verified against the seat keys the tracker pins.
4. Policy and content checks pass, in ljos at export and again in
   mailgate.
5. The owner's yes. The designated mail seat (`mailseat` here) shows the
   whole approval file and `H` on an in-chat approval card, and the owner
   taps yes. Only that seat may call `send`, with the file and the card
   reference.
6. mailgate checks the caller, that the card reference is unused, the
   expiry and the nonce, re-verifies the ballot, runs the policy and
   content checks with its own copy of the deny file, re-hashes the
   message bytes against `H`, and writes them to `sendmail -i -t`
   exactly once. The nonce is spent whether the send succeeds or fails,
   so a retry needs a new tap.

The checks only ever refuse. Passing them grants nothing.

- From must equal the configured `identity`.
- Reply-To, Sender and Return-Path must not name a denied address, domain
  or handle, and no recipient may be at a denied address or domain. A
  trailing dot on a domain is dropped before matching, so
  `user@denied.example.test.` is the denied domain.
- Header fields are unfolded first (a line break before a space or tab is
  removed), and every check reads the unfolded value. A header line that
  is neither a field nor a continuation is refused.
- The draft is one part: exactly one `Content-Type: text/plain;
  charset=utf-8` with no other parameter, and `Content-Transfer-Encoding`
  absent, `7bit` or `8bit`. Multipart, `text/html`, base64,
  quoted-printable and any other encoding are refused, as is `=?`
  (an encoded word) anywhere in the headers or body, and HTML in the
  body: a `<` before `/`, `!` or `?`, or an entity such as `&amp;` or
  `&#101;`. So what the checks scan is what a client shows.
- Subject, headers and body must not carry a denied term, or both halves
  of a denied pair, after NFKC normalisation, case folding and removing
  invisible format characters. The term, pair and attribution scans run
  twice, on the body as written and on a copy with every `<...>`
  removed and numeric references (with or without their `;`) and
  `&amp`, `&lt`, `&gt`, `&quot`, `&apos` and `&nbsp` decoded, and refuse
  when either hits. So `<https://example.test/a>` passes, and
  `pro<span>ject-x` is the term `project-x`.
- Built-in attribution phrases ("generated by", "as an AI", model names,
  tool `X-Mailer` headers) are refused, and `attribution` adds more.

The deny file is one rule per line, `#` starts a comment:

```
address: someone@example.test
domain: example.test
handle: @someone
term: project-x
pair: first term + second term
```

## What each part is worth

The card tap is the owner's yes, but mailgate trusts the mail seat to
relay it, because the tap carries no signature mailgate could verify. The
ballot is real review, but while seats share a Unix user a determined
seat could forge one. The policy and content checks only refuse. Each
send is therefore also recorded in mailgate's audit, mailed to the owner
as a receipt, and compared against Sent Items after each sync, so a send
the owner never approved is caught afterwards.
