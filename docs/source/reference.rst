:description: The ljos command line, the Model Context Protocol (MCP) tools, the git hooks, the rules, the environment and the help text.
:cover: https://leidarljos.github.io/ljos/_static/og.png

=========
Reference
=========


Each page answers one lookup. :doc:`Commands <reference-commands>` is the one most readers want. The Model Context Protocol (MCP) server is ``ljos-mcp``.

- :doc:`Commands <reference-commands>`: every ljos verb, the stores it reads and writes, and what it does.
- :doc:`MCP tools <reference-mcp>`: the tools ``ljos-mcp`` serves, the core set and how to list every tool.
- :doc:`Git hooks <reference-git-hooks>`: the global git hooks that book commits, and how to install, remove and check them.
- :doc:`Environment and sync files <reference-environment>`: the variables the seat reads, a temporary home, and the files ljos sync carries.
- :doc:`Rules and checks <reference-rules>`: user rules, what a rule reads, the checks ljos keeps for the tracker, and the threat model of the pack token.
- :doc:`Jev settings <reference-jev>`: the keys of jev.toml and what each one changes.
- :doc:`The learning rules <reference-learning>`: how trust rows move after an outcome, and the settings that bound them.
- :doc:`Help text <reference-help>`: what ``ljos --help`` prints, taken from this version.

Crates
======

================== =========================================================================
Crate              Carries
================== =========================================================================
``ljos-cli``       the library and ``ljos``
``ljos-mcp``       the MCP server
``ljos-consensus`` DeGroot and Friedkin-Johnsen settles, Seldon export (separate repository)
================== =========================================================================

Cite
====

``CITATION.cff`` at the repository root names leiðarljós (ljos),
its author and this version.

.. toctree::
   :maxdepth: 1
   :hidden:

   reference-commands
   reference-mcp
   reference-git-hooks
   reference-environment
   reference-rules
   reference-jev
   reference-learning
   reference-help
