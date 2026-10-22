# Mail

A seat may read the owner's mail and may draft mail in the owner's name. It
must never send on its own. Reading goes through two opt-in MCP tools over a
local notmuch database. Sending goes through a separate Unix user,
`mailgate`, that alone holds the mail password and the send path, and that
sends one exact message only after a ballot, the content checks and the
owner's own yes. Everything specific to one machine (addresses, hosts,
password entries, wrapper paths, the deny file and route rules) lives in
config, never in this repository.

## Config

The `[mail]` table is optional. Without it the mail tools refuse and
nothing else changes. It is read from the same layers as the rest of ljos
config: `~/.config/vissue/config.toml`, `~/.config/ljos/ljos.toml`,
`~/.config/ljos/mail.toml`, then `./vissue.toml`, `./ljos.toml`,
`VISSUE_CONFIG` and `LJOS_CONFIG`. A later file overrides the keys it sets.
A file that does not parse, or a `[mail]` key ljos does not know, is an
error rather than a silent default.

```toml
[mail]
# NOTMUCH_CONFIG for every read. Unset reads notmuch's own default.
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
`LJOS_MCP_TOOLS=core,mail_search,mail_show`. A cloud seat (one that ran
`ljos cloud up`) gets a refusal: it has no database, and personal mail
stays on the machine that holds it.

- `ljos_mail_search {query, limit}` runs notmuch with a fixed argv. The
  query is one argv element after `--`, so it cannot become an option. The
  limit is at most 50. It returns thread id, date, tags and counts, with
  subject and authors wrapped as untrusted. Mail tagged `spam` or `deleted`
  is left out unless the query names that tag.
- `ljos_mail_show {thread}` takes a 16-digit thread id. Text parts are
  wrapped as untrusted. HTML is reduced to the text a mail client would
  show: `display:none`, `visibility:hidden` and the `hidden` attribute drop
  their text, and the message is flagged when they did. Links are shown as
  text with their target, never followed. Attachments are listed by name,
  type, size and sha256, with no content and no path.

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
   `.` as text, and `-t` takes the recipients from `To` and `Cc`.
2. Export. The approval file is plain text: a statement block, a blank
   line, then the message bytes.

   ```
   ljos-outbox v1
   draft <outbox draft id>
   reachbook-hash <the outbox's own message hash>
   expires <RFC 3339, at most 24 h out>
   nonce <16 random bytes, hex>

   <the message, LF line endings>
   ```

   `H` is the sha256 of the message bytes after the statement block,
   which are exactly the bytes mailgate writes to `sendmail` stdin.
   mailgate hashes the bytes it is about to write and refuses on a
   mismatch with `H`. The expiry and nonce are not in `H`; mailgate
   checks them and spends the nonce. The exporter refuses control
   characters other than tab and newline, and the bidirectional overrides,
   anywhere in the message, so a terminal cannot hide text from the
   person reading it.
3. Consensus. The draft's send issue carries the option `send H`. It
   passes with ballots from at least two seats of at least two model
   families, each verified against the seat keys the tracker pins.
4. Policy and content checks pass, in ljos at draft time and again in
   mailgate.
5. The owner's yes. The designated mail seat (`mailseat` here) shows the
   whole approval file and `H` on an in-chat approval card, and the owner
   taps yes. Only that seat may call `send`, with the file and the card
   reference.
6. mailgate checks the caller, that the card reference is unused, the
   expiry and the nonce, re-verifies the ballot, runs the policy and
   content checks with its own copy of the deny file, re-hashes the
   message bytes against `H`, and writes them to `sendmail -i -t`
   exactly once. The nonce is spent whether
   the send succeeds or fails, so a retry needs a new tap.

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
  body: a `<` before a letter, `/`, `!` or `?`, or an entity such as
  `&amp;` or `&#101;`. So what the checks scan is what a client shows.
- Subject, headers and body must not carry a denied term, or both halves
  of a denied pair, after NFKC normalisation, case folding and removing
  invisible format characters.
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
