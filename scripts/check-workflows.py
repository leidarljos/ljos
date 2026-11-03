#!/usr/bin/env python3
"""Fail when a workflow file cannot be trusted to run as written.

GitHub rejects a file that repeats a mapping key, so a tag push makes
no release. A dtolnay/rust-toolchain toolchain value other than the
rust-toolchain.toml channel installs a compiler cargo does not run.
Two actions/checkout shas in one file let one job drift from the others.
"""
import pathlib
import re
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

# A checkout pin is the action repository's commit: a SHA-1 object id,
# forty hex digits. A ref such as @v4 is not a second pin.
CHECKOUT_SHA = re.compile(r"^actions/checkout@([0-9a-f]{40})$")
# The release workflow installs the channel a step read from the pin.
CHANNEL_EXPR = re.compile(
    r"^\$\{\{\s*steps\.([A-Za-z0-9_-]+)\.outputs\.channel\s*\}\}$")
CHANNEL_LINE = re.compile(r'(?m)^channel *= *"([^"]+)"\s*$')


def read_channel():
    path = pathlib.Path("rust-toolchain.toml")
    try:
        text = path.read_text()
    except OSError as err:
        print(f"{path}: {err}")
        return None
    found = CHANNEL_LINE.findall(text)
    if len(found) != 1:
        print(f"{path}: expected one channel line, found {len(found)}")
        return None
    return found[0]


def line_of(node):
    return node.start_mark.line + 1


def scalar(node):
    if isinstance(node, yaml.ScalarNode):
        return node.value
    return None


def get(map_node, key):
    for key_node, val_node in map_node.value:
        if scalar(key_node) == key:
            return val_node
    return None


def run_reads_channel(step):
    text = scalar(get(step, "run"))
    if not isinstance(text, str):
        return False
    # The step assigns the channel line of rust-toolchain.toml and
    # prints it, so the action installs that channel and not a second pin.
    return "rust-toolchain.toml" in text and re.search(r"(?m)^channel=", text)


def job_steps(job):
    # Step ids are unique inside one job. A step in another job is a
    # different step, even when the id text matches.
    steps = get(job, "steps")
    if not isinstance(steps, yaml.SequenceNode):
        return []
    return [item for item in steps.value if isinstance(item, yaml.MappingNode)]


def check_tree(path, root, channel):
    messages = []
    checkouts = []
    if not isinstance(root, yaml.MappingNode):
        return messages
    jobs = get(root, "jobs")
    if not isinstance(jobs, yaml.MappingNode):
        return messages
    for _, job in jobs.value:
        if not isinstance(job, yaml.MappingNode):
            continue
        steps = job_steps(job)
        by_id = {}
        for step in steps:
            step_id = scalar(get(step, "id"))
            if isinstance(step_id, str):
                by_id.setdefault(step_id, []).append(step)
        for step in steps:
            uses = get(step, "uses")
            uses_text = scalar(uses)
            if not isinstance(uses_text, str):
                continue
            checkout = CHECKOUT_SHA.match(uses_text)
            if checkout:
                checkouts.append((checkout.group(1), line_of(uses)))
            rust = (
                uses_text == "dtolnay/rust-toolchain"
                or uses_text.startswith("dtolnay/rust-toolchain@"))
            if not rust:
                continue
            with_node = get(step, "with")
            if not isinstance(with_node, yaml.MappingNode):
                continue
            tool = get(with_node, "toolchain")
            if tool is None:
                continue
            value = scalar(tool)
            if channel is not None and value == channel:
                continue
            expr = CHANNEL_EXPR.match(value) if isinstance(value, str) else None
            if expr and any(
                    run_reads_channel(s) for s in by_id.get(expr.group(1), [])):
                continue
            shown = value if isinstance(value, str) else "not a string"
            where = line_of(tool)
            if channel is None:
                messages.append(
                    f"{path}:{where}: toolchain {shown!r} has no channel to match")
            else:
                messages.append(
                    f"{path}:{where}: toolchain {shown!r} "
                    f"is not rust-toolchain.toml channel {channel!r}")
    if len({sha for sha, _ in checkouts}) > 1:
        pin, pin_line = checkouts[0]
        for sha, lineno in checkouts[1:]:
            if sha != pin:
                messages.append(
                    f"{path}:{lineno}: actions/checkout@{sha} "
                    f"is a second checkout sha; line {pin_line} pins {pin}")
    return messages


def load_workflow(text):
    # One parse. Constructing the node applies the repeated-key check.
    # A syntax error is raised once, from this parse, not from a second one.
    loader = Strict(text)
    root = loader.get_single_node()
    if root is not None:
        loader.construct_document(root)
    return root


def main():
    channel = read_channel()
    bad = channel is None
    workflow_dir = pathlib.Path(".github/workflows")
    paths = sorted(
        list(workflow_dir.glob("*.yml")) + list(workflow_dir.glob("*.yaml")))
    for path in paths:
        text = path.read_text()
        try:
            root = load_workflow(text)
        except yaml.YAMLError as err:
            print(f"{path}: {err}")
            bad = True
            continue
        if root is None:
            continue
        for message in check_tree(path, root, channel):
            print(message)
            bad = True
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
