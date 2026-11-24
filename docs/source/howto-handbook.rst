:description: Publish the lessons, policies, tickets and workflows you mark public as Org.
:cover: https://leidarljos.github.io/ljos/_static/og.png

====================================
Keep a project handbook in your repo
====================================


Mark what may leave
===================

The handbook is opt-in. Nothing leaves your machine unless you marked it, and
an unmarked item stays out whatever its text says.

- A lesson, a preference or a rule is marked when it is written:
  ``ljos remember --public TEXT``, ``ljos prefer --public TEXT`` or
  ``ljos rule --public ...``. A row that already exists is marked with
  ``ljos memories review --public ID``. Repeating a private lesson with
  ``ljos remember --public`` marks that row public. ``ljos memories review --public``
  records a project from ``-p`` or the checkout when the row names none.
  The mark is the pack entity ``visibility:public``. A reviewed edit of a lesson
  takes the mark off.
- A ticket is marked with the tag ``public``. Marking it publishes its title,
  body and notes.
- A ticket the page leaves out is named nowhere. Every string the page prints
  is read for its id: lessons, preferences, rules, workflow names and
  titles, ``--title``, and a ticket's title, body and notes. The tracker's
  tickets are loaded for this whenever the checkout has a tracker, with or
  without ``--tree``. The id shows as ``external dependency``. The text is read
  as the screen below reads it, so a look-alike hyphen or dash, a zero-width
  mark, a full-width letter and one layer of percent, HTML, ``\u`` or
  quoted-printable escape do not hide an id.
  Org emphasis marks around the id, a line break next to its hyphen and an
  underscore for the hyphen do not hide it either, and an id with a suffix
  (``kiln-quiet-followup``, ``kiln-quiet_2``) is still that id. A token is also
  replaced when it is the name of a project the tracker or the router knows,
  a hyphen, and exactly ``id_length`` characters from ``0-9`` and ``a-z`` (4, or
  the ``id_length`` under ``[issues]`` in ``vissue.toml``), even when no ticket
  has that id. Letters alone count, so a word of that length after a
  project name is replaced too. Other hyphenated words (``iso-8601``,
  ``x86-64``, ``CVE-2024-3094``) are kept. A marked ticket of the project keeps
  its id.
- A workflow file is marked with one line, written exactly ``#+public: yes``,
  in its header, the run of ``#+`` lines at the top of the file. The line is
  compared byte for byte: another case (``#+PUBLIC: yes``, ``#+public: YES``),
  another value (``t``, ``true``), no space, a tab or a trailing space after the
  colon, and a CR from CRLF line endings do not mark the file. A plain
  ``public: yes`` line, a line further down, and a line inside a block such as
  ``#+begin_example`` do not mark it either. The index lists the file name and
  the ``#+title``, read from the same header.

The default is private everywhere. ``--public`` also records the project, from
``-p`` or from the checkout, because the handbook is limited to one project.
Marking is the person's to give, so a runner cannot: ``remember --public``,
``prefer --public``, ``rule --public`` and ``memories review --public`` each exit 1
under a runner and write nothing.
A guard row, such as the protected branches or the push quorum of a checkout,
is not a lesson and is never written.

Write the handbook
==================

Run the command from the project checkout. ``--project`` is required. Repeated
``--tag`` values narrow that project: every tag has to be present, and a bare
word is not a tag. A marked item that also names another project is left out.

Lessons are the marked ones kept for the project. A dropped review is left
out even when the row was stored as explicit. Text stays as it was written,
except that a tab or another control character becomes a space. A line that
would open a heading, a keyword, a drawer, a block, a table, a rule, a
comment, or a ``SCHEDULED:``, ``DEADLINE:`` or ``CLOSED:`` planning line gets a
backslash in front, and so does a zero-width or fullwidth lookalike of one.
Tracker bookkeeping is left out, and the page does not name who noted, claimed
or finished a ticket. A state change has the form ``State "DONE" from "TODO"``.
A clock line is ``CLOCK:`` followed by an org time. A noted, claimed or finished
line names one seat and nothing else. A sentence that only begins with those
words is kept.

The title is the fixed text ``Project handbook``. ``--title`` replaces it, and a
title that reads as private refuses the export. A heading comes from a marked
item or from a fixed word, never from an unmarked ticket, lesson or workflow.

.. BEGIN ljos-sample handbook

.. code:: console

   $ ljos handbook --project kiln --tree kiln-a --title "Kiln release notes" --out -
   #+title: Kiln release notes
   #+options: toc:2 num:nil
   #+TODO: TODO | DONE CANCELLED

   * Project knowledge
   ** Lessons
   - Publish the signed archive.

   * Policies
   ** Standing preferences
   - Count the samples twice.

   * Release tickets
   ** TODO kiln-a Cut the release
   - Cut the archive on a Tuesday.

   ** Dependency graph
   #+begin_src dot
   digraph release {
     "kiln-a";
   }
   #+end_src

   ** Release order
   - kiln-a

   * Workflows
   ** Release train
   - [[file:release.org][release.org]]

.. END ljos-sample handbook

Policies are the marked preferences and command rules of the project.

``--tree`` follows blockers inside the project, through marked tickets only.
The root has to be marked: an unmarked root is an error. A parent link nests
a ticket and does not order it. A blocker that is in another project or is
not marked is one line, ``external dependency``, with no id and no title. Each
cycle is named on its own ``Cycle:`` line and the command still exits 0.
``CANCELLED`` stays cancelled.

``--workflows`` reads the marked Org files for their file name and ``#+title``
and links each one. The files are not rewritten. With no directory named,
``.ljos/workflows`` is read when it is there. A directory ``--out`` writes
``faq.org`` and still includes the ticket tree and the workflow index.
``--out /dev/stdout`` is the same as ``--out -``.

``--out`` is refused, with the path named and the file left as it was, when it
is a symbolic link (whether it resolves or dangles), is not a regular file or
is read-only. A directory that cannot be written is named in the error. Name
the file a link points to, or make the file writable, and run the command
again.

``--out`` writes a temporary file beside the target, syncs it and renames it
over the target. A write that fails or is cut short leaves the earlier file
whole.

A marked item that reads as private refuses the export
======================================================

The text of a marked item still goes through one public-text screen. It is a
backstop and not the control: the mark is the control. When a marked item
matches a private pattern, the whole export is refused. The command writes
no file and no output, prints one line for each refused item, and exits 1:

::

   handbook: refused lesson 2f3c...: term
   handbook: nothing was written, because 1 marked item is not public-safe

Each line names the kind and the id of the item and one class: ``term``,
``seat``, ``path``, ``secret``, ``address``, ``email``, ``org-markup`` or
``character``. The text that
matched is never printed. An id that reads as private is shown as
``id withheld``. Nothing is redacted, and an existing ``--out`` file is left as
it was. Reword or unmark the item and run the command again. The export does
not read ``LJOS_SEAT`` or any seat of the process running it: the same tracker
gives the same bytes under any seat name.

The screen catches accidental leaks in text a person or an agent wrote
normally: paths, private addresses, email addresses, secrets and banned terms.
A path, an email address or a secret cut by an ordinary line wrap is still
found. The screen is no defence against an author who deliberately obfuscates
text. Nested encodings, homoglyph spellings of banned words, escapes split
across lines and addresses broken across lines are outside its threat model.

Class-specific scanners read words, paths, links, numbers and punctuation
directly from one canonical copy of the text. Their context checks distinguish
a version such as ``3.12.3`` or a number such as ``139.1`` from an address. This
is the same canonical reading that ``ljos workflow render`` uses. One layer of
percent, HTML (numeric and named),
``\u``, ``\x`` and quoted-printable escapes is decoded. The decoded text is
put through NFKD, zero-width and bidi controls are removed, every Unicode line
break is read as a line break and every space separator as a space. The screen
scans each line, and then lines joined across an ordinary wrap for paths,
email addresses, secrets and terms. It does not join an escape such as ``%2``
on one line and ``F`` on the next.

The public character set is the backstop for odd characters. A character
outside it, written or decoded from an escape, is refused as ``character``,
with no placeholder. The set is printable ASCII, the line break, and the
non-ASCII characters used by these docs: ``·``, ``é``, ``ð``, ``ó``, ``ʼ``,
``…`` and ``−``. A base letter with combining marks counts as the letter it
composes to. ``ljos workflow render`` calls the same classifier and therefore
refuses the same text.

A credential is refused when a name such as ``token``, ``secret``, ``password``,
``api_key`` or ``Authorization`` is followed by ``=`` or ``:``, with any spaces
around the mark, and when the name is joined to its value by a word or an
arrow, as in ``the password is hunter``, ``password => x`` or ``the pin is 4921``.
A prose placeholder such as ``key=value`` stays.
A name made of one of those words, such as ``SECRET_KEY_BASE``, ``DB_PASS``,
``MYSQL_PWD`` or ``--password``, counts too, and so does a login on a command
line (``curl -u user:pass``, ``sshpass -p``), a ``user:pass@`` in a URL, a glued
``-p`` value, a ``Cookie:`` header, ``Bearer`` with a long word, ``Basic`` or ``token``
with an encoded value, an AWS key id, a vendor key prefix, and a run of 40
characters or more that looks random. The export and the prompt excerpt that
``ljos remember`` keeps use one matcher for a word. Besides the formats above it
refuses a word that starts with a vendor key prefix (the prefixes of a code
host token, a chat token and a cloud key id), also when quotes, brackets or
Org marks wrap it, an assignment of a value to a name such as ``token`` or
``secret`` with a value after it (also with a doubled mark or a double colon), and a
run of 24 or more letters and digits with both kinds in it. A run that ``-``,
``.`` or ``_`` cut into pieces counts when one piece is itself a mixed run of 8
or more, and a run cut by a line break counts when neither half is 24 long
alone. A capitalised name before a double colon, as in a Rust path, is a path. The
excerpt also redacts a marker with nothing after it, and a word that dashes
cut into short pieces. The export reads those as prose. A hex run of 32 or more is a key,
except a lowercase one of exactly 40 or 64 digits, or one that follows a hash
name such as ``sha256``. A PEM private key is refused with every line between
its ``BEGIN`` and ``END`` markers. A word that Org shows as verbatim, such as
``token`` or ``secret``, is the word and not a credential.
The phrase check reads a word that ends in ``ed``, ``ing`` or ``ly`` after
``is`` as prose, so a password spelled like a participle is not detected.

A path, a ``file:`` link with an absolute or ``~/`` target, and a seat phrase are
refused. The commands of the system, such as ``/usr/bin/env``, and the files
and folders ljos names for itself are not private. Banned product and vendor
terms are refused in any case and with any spacing. The words of ljos itself,
such as ``seat``, ``hook`` and ``mcp``, are refused because the page is public.

One range test decides a non-public address: loopback, unspecified,
private IPv4, link-local IPv4, IPv6 link-local ``fe80::/10``, unique local
``fc00::/7``, site-local ``fec0::/10``, multicast, the documentation ranges
192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24 and ``2001:db8::/32``,
benchmarking, carrier-grade NAT ``100.64.0.0/10``, and the IPv4-mapped or
IPv4-compatible form of any of those. A zone suffix after ``%`` does
not hide the address. Letter case does not change the parse.
The screen reads every run of hex digits, colons and dots, cuts it at every
start and every end, and asks the standard IPv4 and IPv6 parsers about each
piece. No character before or after a literal hides it, so ``10.0.0.1x``,
``a10.0.0.1``, ``1.10.0.0.5``, ``fd00::1234abc``, ``a.fe80::1``, ``crate::fd00::1``
and ``add.fe80::1%eth0`` are refused. A piece is at most 45 characters, the
longest address the parsers read, so a run of any length is read piece by
piece, and an OID, a dot leader or a dotted hex dump is refused only when a
piece of it is an address. Six or more two-digit hex groups split by single
colons, as a MAC address or a key fingerprint is written, are refused only
for a non-public piece, with or without a full stop after them. A piece in a non-public range
is refused, and no path, version or word rule spares it, so ``fd00:Beef::1``,
``fe80::1``, ``2001:db8::Dead`` and ``v10.0.0.1`` are refused.
The one exception is a lone double colon with no hex digit, colon or dot
touching it. It is punctuation at the start of a line or after a space, a
slash, an opening parenthesis or bracket, a comma, a quote, a backtick or a
tilde, as in ``term :: description``, ``Notes ::`` and ``x/::``. It is also
punctuation after a word with a letter other than a to f when a space or the
end of the line follows, as in ``term:: description``, and in a Rust path.
After an equals sign or a dot it is the unspecified address, so
``addr=::`` and ``ip=::`` are refused.
A public IPv6 piece is refused when it has a digit, or when it has none and
opens or closes with a double colon or has two groups of four hex letters,
as ``dead::beef`` and ``::a`` do. A Rust path spares it. The left side of a path
is an identifier with a letter, or the ``>`` that closes generics. The right
side starts with ``<``, ``{`` or a capital letter, or is a lowercase name that
``(`` follows or that sits in a path with a letter other than a to f. So
``Face::Fade``, ``dead::Beef``, ``f64::E``, ``d3::<T>``, ``use bb8::{Pool}``,
``std::fmt`` and ``Face::<T>::new()`` stay. An IPv4
address is read as the C library's ``inet_aton`` reads it. Each run of numbers
joined by single dots is cut into parts, and each part is read once: hex
after ``0x``, octal after a leading zero, or decimal, of any length. A part
ends before a letter that is not one of its digits, so ``0177.0.0.1x`` is
read. A run of four parts is a quad, and in a longer run every four parts
in a row are read as a quad, so ``1.012.0.0.1``, ``9.0x0a.0.0.1`` and
``1.10.0.0.5`` are refused. Every private quad is refused, in any run and
even in version context, so ``v10.0.0.1`` is refused. A run of exactly four
parts that is public is refused unless it is a resolver, a netmask or a
version-context quad written in decimal. A netmask is ones then zeros, as
in 255.255.240.0, so 255.0.255.0 is refused. Version context is a version
word before or after the quad, or a ``v`` glued to it, as in ``v1.10.0.0``.
Four public parts inside a longer run, as in the OID ``1.3.6.1.4.1.311``, stay.
A private number of one to three parts, read with the last part filling the
bytes left, or a bare integer,
is an address only beside an address word, a port, ``//`` or ``@``, as in
``ip 10.1``, ``ping 167772161`` or ``127.1 host``. The address word, ``//`` and ``@``
may come before or after the number, so ``10.1@`` and ``10.1//`` are refused.
A letter directly before a private dotted quad does not hide it, so
``x10.0.0.1`` is refused. A leading zero does not make it an address,
so ``10.05``, ``10.00`` and ``Ubuntu 10.04 LTS`` stay. A number may start after
a dot that follows a letter, so ``e.g.10.0.0.1`` is refused.
Addresses are
found within one line. An address broken across lines on purpose is not
caught. An email address is refused, and so is one written with the at sign
spelled out in words, brackets or parentheses.
``GitHub`` is a forge name, not a vendor of models or agent tools, and it
stays.

Org syntax that would run or export something is refused whole: a ``#+``
keyword line other than a block that only shows text, a macro, a ``shell:`` or
``elisp:`` link, a script tag, an export snippet, and a call to a babel block,
whether written as ``src_NAME{`` or ``call_NAME(``. A word such as ``src_dir`` or
``call me`` is plain text and stays.

The screen does not catch every phrasing. A secret given as ``the key is abc``
or ``login is admin / hunter2``, or a bare ``db password abc``, is not detected.
Mark only what you would publish.

An error names the flag and never repeats the value you passed, such as a
``--date`` text, a ticket id or a file path. There is no allow list for words
or for email addresses, and no flag keeps a private pattern: the same checks
run on every export.

``--ids`` writes each lesson and policy as a heading with an ``:ID:`` property,
and gives each ticket one. Without it, a lesson or a policy is a list item
with no id. The ids the export writes are not checked as secrets when they
are hex digits, so a pack lesson id is written. Any other id that matches a
private pattern is not written. ``--date`` takes ``YYYY-MM-DD`` and refuses a day
that does not exist, such as ``2026-02-30`` or year ``0000``.

Check the committed file
========================

``--check`` builds the same document in memory and exits 1 when any byte
differs. It also exits 1 when a line in the file, or in a marked workflow
file, matches a private pattern. The message names the file and the line
number and does not print the line. An unmarked workflow file is not read. An
empty file fails. An unknown project fails. The command does not rewrite the
file.

.. code:: console

   $ ljos handbook --project kiln --out release.org --check
