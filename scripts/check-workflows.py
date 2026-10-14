#!/usr/bin/env python3
"""Fail when a workflow file repeats a mapping key.

GitHub rejects such a file without running it, so a tag push makes no release.
"""
import pathlib
import sys

import yaml


class Strict(yaml.SafeLoader):
    pass


def mapping(loader, node, deep=False):
    seen = set()
    for key_node, _ in node.value:
        key = loader.construct_object(key_node, deep)
        if key in seen:
            raise yaml.constructor.ConstructorError(
                None, None, f"repeated key {key!r}", key_node.start_mark)
        seen.add(key)
    return yaml.SafeLoader.construct_mapping(loader, node, deep)


Strict.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, mapping)

bad = 0
for path in sorted(pathlib.Path(".github/workflows").glob("*.yml")):
    try:
        yaml.load(path.read_text(), Strict)
    except yaml.YAMLError as err:
        print(f"{path}: {err}")
        bad = 1
sys.exit(bad)
