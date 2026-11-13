:description: Bump an EasyBuild recipe with eb-stack in a sitting, plan a whole generation, and keep what the campaign taught.
:cover: https://leidarljos.github.io/ljos/_static/og.png

===========================
Bump a recipe with eb-stack
===========================

.. raw:: html

   <span id="bump-a-recipe-with-eb-stack-and-keep-what-the-campaign-taught"></span>

One sitting on the ticket, one island per recipe before it is touched:

.. code:: bash

   ljos sitting demo-319z
   ljos island "eOn 2.17.10 foss 2026.1"

``out`` is a directory eb-stack wrote, with ``locks/default.lock.json`` and
``package.sbom.cdx.json`` inside it, and ``out/campaign.json`` when the campaign
has run. Without those files the command names the missing path.
Then the ladder, each rung its own artifact: ``eb-stack recipe check``,
``eb-stack package bump`` (the lock under ``out/locks`` is ``resolves``),
``eb-stack recipe lint``, ``eb-stack target doctor``, ``eb-stack campaign run`` and ``campaign status`` (``builds``, ``binary-verified``). Over MCP the
same verbs are ``eb_recipe_check``, ``eb_package_bump``, ``eb_recipe_lint``,
``eb_target_doctor``, ``eb_campaign_run``, ``eb_campaign_status``, beside
``ljos_sitting``, ``ljos_island``, ``ljos_remember`` and ``ljos_finish``.

For a whole generation, put the bundle on the tracker before anybody
builds, and let the graph hand out the modules, each up the same ladder:

.. code:: bash

   ljos bump-plan out --project demo --parent demo-319z --dry-run
   ljos bump-plan out --project demo --parent demo-319z
   vissue ready -p demo

The plan writes one child issue per module the lock builds, blocked by
the modules built before it; ``ready`` is the frontier a seat can sit on, and ``ljos sitting``
refuses a module whose blockers are still open. The ids are a hash of
the module and the generation, so running it again after the bundle
changed adds the new modules and keeps the rest.

When the campaign ends, its typed findings are the lessons:

.. code:: bash

   ljos findings out/campaign.json
   ljos findings out/campaign.json --remember --issue demo-319z

The first prints one line per finding. The second stores one lesson per
finding a person or a seat resolved, at once, under the recipe's name, the
package and the failure class, with origin ``tool-echo`` at L0 (``derived``).
``ljos accept ID`` raises it to ``explicit``, and the command cites the
state file on the issue as a deed. A finding a later attempt merely got
past is skipped; ``--all`` takes it too. The recipe's next bump recalls
this lesson from its island.

::

   GCCcore-15.2.0 on terra: compile failed in the build step with fatal error: linux/scc,h. Fix: applied the GCC 14 libsanitizer kernel headers patch in GCCcore-15.2.0.
