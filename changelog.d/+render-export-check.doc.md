The workflow how-to and `workflow render --help` describe a refusal, and the doc export check fails when `docs/source/howto-workflow.rst` drifts from the pandoc export.
The check also finds Org pages that changed without their reStructuredText page.
The base exporter exports both revisions of each such page, and the check fails if they differ.
