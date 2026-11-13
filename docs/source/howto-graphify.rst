:description: Install graphify, build graphify-out/graph.json, and read the code context ljos recall adds.
:cover: https://leidarljos.github.io/ljos/_static/og.png

======================
Use ljos with graphify
======================

graphify maps a repository into ``graphify-out/graph.json``.
ljos remembers what agents learned and tracks their work.
When that file is present, ``ljos recall`` adds a code context section from it.
ljos does not run graphify, and the section does not use the network.

Install both
============

Graphify needs Python 3.10 or newer.
The package on PyPI is ``graphifyy``. The command is ``graphify``.
The source is `Graphify-Labs/graphify <https://github.com/Graphify-Labs/graphify>`__, Apache-2.0.
The ``graphify install`` command registers the ``/graphify`` skill with the assistant it detects.
A named platform is the same command with a flag, for example ``graphify cursor install`` or ``graphify install --platform codex``.

.. code:: console

   $ uv tool install graphifyy
   $ graphify install

``pipx install graphifyy`` is the other install.
If neither ``uv`` nor ``pipx`` is a command, ``python3 -m pip install --user graphifyy`` installs ``graphify`` into ``~/.local/bin``.
The ljos installer adds that directory to ``PATH``.
ljos is installed on its own. :doc:`Getting started <getting-started>` covers that.

Build the graph
===============

In the repository, in the assistant:

.. code:: text

   /graphify .

Codex uses ``$graphify`` for the same skill.
With no assistant, ``graphify update .`` writes the same files and does not call a model.
``graphify install`` with no assistant found still writes ``~/.claude`` and tells you to type ``/graphify .``.
That directory is enough for ``ljos onboard`` to try to register Claude Code.
If ``claude`` is not a command, the mcp line names that and the command to run once it is installed.
The run writes three files under ``graphify-out/``: ``graph.html``, ``GRAPH_REPORT.md`` and ``graph.json``.
``graph.json`` is a NetworkX node-link graph.
``graph.schema_version`` is the major. This ljos reads major 1.
``graph.graphify_version`` names the graphify release that wrote the file.
A later major is refused. The line names the major, and the rest of the recall still prints.
Edges are tagged ``EXTRACTED`` (read from the source) or ``INFERRED`` (resolved by graphify). ``AMBIGUOUS`` is the third tag.

Read it from recall
===================

.. code:: console

   $ ljos recall ljos-ab12

The working set is unchanged.
When ``graphify-out/graph.json`` is in the repository and a node matches the issue id, its title or its tags, a Code context section follows the memories.
A node line has the label, the source file and line, a community id and the degree.
Under it are a few edges. Each edge names the other node, the relation and the tag.
No graph file means no section. Recall does not parse a file in that case.

``ljos recall ljos-ab12 --no-graph`` leaves the section out.
``LJOS_GRAPHIFY=off`` does the same.
``LJOS_GRAPHIFY=/path/to/graph.json`` reads that file instead.
A ``graphify`` line in ``ljos.toml``, or in ``$LJOS_CONFIG``, set to a path, does the same.
``off`` in that key leaves the section out.
A relative path is from the file that set it. A relative ``LJOS_GRAPHIFY`` is from the working directory.
A path that is not a file is the same as no graph.

The search walks from the working directory up to the repository root, and stops at ``.git``.
The Model Context Protocol tool ``ljos_recall`` adds the same section.
It follows ``LJOS_GRAPHIFY``. It has no separate flag.
