:description: Define a versioned Org skeleton, file one ticket per step, and close the release on a deed plus a ship consensus.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=====================================
Run a recurring release as a workflow
=====================================


A release is the same DAG every time: changelog, version bump, tests, docs, tag, artifacts, a packaging notice, review, and a sign-off. The skeleton is an Org file in the repository. Each run is a parent ticket and one child per step.

Store the skeleton
==================

The file name is the skeleton name. ``ljos workflow define docs/examples/workflows/release.org`` checks the DAG and stores the bytes as a deed, with the sha256 and the ``#+version``. The same bytes again change nothing. A changed body needs a higher ``#+version``. A dependency listed twice in ``:DEPENDS:`` is refused when the skeleton is defined.

.. code:: console

   $ ljos workflow define docs/examples/workflows/release.org

File a run
==========

File the parent, then start the skeleton under it. ``--var version=1.2.0`` fills ``{{version}}`` in the step titles and bodies. The tracker records a block edge for each ``:DEPENDS:``. Running the same command again files nothing new and names the tickets already there.

.. code:: console

   $ ljos file "Release 1.2.0" -p demo
   $ ljos workflow --start release PARENT --var version=1.2.0

``ljos workflow PARENT`` prints each step, the steps whose dependencies are done, the open blockers, and what the sign-off still needs. ``ljos ready`` lists those same unblocked steps, because a step waits on its dependencies. A parent that already runs a skeleton cannot start the feature workflow, and a parent that already runs the feature workflow cannot start a skeleton. Nothing is written on that refusal.

.. code:: console

   $ ljos workflow PARENT
   $ ljos ready

A problem found while a step is open is a child of that step, or a ``ljos block`` edge to another ticket inside or outside the run. Both block the step, appear in ``ljos workflow PARENT``, and hold the sign-off until closed. A ``:BLOCKED_BY:`` id that has no ticket is reported as missing but blocks nothing. This matches ``vissue ready``, and both ready lists still include the step. Concurrent ``--start`` calls on the same parent wait up to thirty seconds on a per-run lock so each step is filed once. ``LJOS_WORKFLOW_LOCK_WAIT`` is that wait in seconds when it is a positive integer. A caller that cannot acquire the lock exits with the parent and lock path.

.. code:: console

   $ ljos file "CI failed on the tag step" --parent STEP
   $ ljos block STEP --on OTHER

Sign off
========

The sign-off step stays open until every other step is done, no blocker of any step is open, ``ljos consensus STEP --quorum 2`` settles ship, and at least one deed named on the step passes ``ljos evidence``. The quorum is the ``:QUORUM:`` property, and 2 when the skeleton omits it. ``:DEED: yes`` is what asks for the deed. The kind, the dependencies, the quorum and the deed are read from the ticket tags the start command writes. A later note does not change them. ``ljos complete`` and ``ljos finish`` both refuse, exit 1, and list what is still missing.

.. code:: console

   $ ljos consensus SIGNOFF --quorum 2
   $ ljos deed SIGNOFF --add ACCESSION
   $ ljos evidence ACCESSION

Revise the skeleton
===================

Notes and blockers from a finished run become a proposal. The proposal is a decision whose options are keep and revise. After ``ljos consensus`` settles revise, applying a file stores the next version, notes the proposal and closes it. Applying the same body again does not store another version.

.. code:: console

   $ ljos workflow revise release --from PARENT
   $ ljos consensus PROPOSAL
   $ ljos workflow revise release --apply FILE

Publish the page
================

``ljos workflow render release --file docs/examples/workflows/release.org --out docs/examples/workflows/release-public.org`` writes the steps in dependency order and the edges as plain text. It emits author text unchanged. Workflow render calls the same classifier over the same canonical text as ``ljos handbook``. The handbook how-to defines that classifier, its public character set and its accidental-leak threat model. A hit names every source line and class, and render writes nothing. A prose placeholder such as ``key=value`` stays. A ``--var`` argument that is not key=value is refused without repeating the argument. A ``--var`` value that contains a line break or a control character is refused before render fills it.

Private dotted IPv4 and IPv6 addresses are sensitive. Every private dotted quad is refused, even beside a version word or a glued ``v``. A public dotted quad is refused unless it is a resolver, a netmask or a version-context quad written in decimal. A private number of one to three parts, or a bare integer, stays unless an address word, a port, ``//`` or ``@`` comes before or after it. A letter directly before a private dotted quad does not hide it. An IPv6 literal that opens or closes with a double colon is refused unless it is a Rust path or a lone double colon between spaces. A netmask is ones then zeros, as in 255.255.240.0. Public Org links to https stay intact. A title or body that starts an Org heading, keyword or drawer is refused. A ``:ROLE:`` value that names a declared seat is refused. Step keys are checked when the skeleton is parsed. ``--check`` exits 1 when the file differs from a fresh render or contains a detected line.

.. code:: console

   $ ljos workflow render release --file docs/examples/workflows/release.org --out docs/examples/workflows/release-public.org
   $ ljos workflow render release --file docs/examples/workflows/release.org --out docs/examples/workflows/release-public.org --check
