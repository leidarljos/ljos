#!/usr/bin/env python3
"""Exercise command consent through the MCP wire protocol on isolated stores."""
import json
import os
from pathlib import Path
import selectors
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

BINARY = str(Path(sys.argv.pop(1)).resolve())
REQUEST_ID = "1" * 32
THREAD = "consent-test-thread"
COMMAND = "printf consent-test"


class ConsentWireTest(unittest.TestCase):
    replace_binary = False
    protocol = "2025-11-25"

    def exchange(self, *, answer=None, capabilities=None, session=THREAD,
                 age=0, change=None, arguments=None, server_seat="consent-test"):
        with tempfile.TemporaryDirectory(prefix="ljos-consent-") as tmp:
            root = Path(tmp)
            store = root / "ljos" / "approvals"
            store.mkdir(parents=True, mode=0o700)
            path = store / "requests.json"
            request = {
                "id": REQUEST_ID, "created": int(time.time()) - age, "approved": False,
                "scope": {
                    "session": THREAD, "shape": "DenyOnly", "tool": "Bash",
                    "cwd": tmp, "command": COMMAND, "pattern": "printf*",
                    "reason": "The test requires explicit consent.",
                },
            }
            path.write_text(json.dumps([request]))
            path.chmod(0o600)
            tracker = root / "tracker"
            tracker.mkdir()
            (tracker / "vissue.toml").write_text('seats = ["consent-test"]\n')
            env = dict(os.environ, XDG_RUNTIME_DIR=tmp,
                       VISSUE_ROOT=str(tracker), ISSUE_ROOT=str(tracker))
            if server_seat is None:
                env.pop("LJOS_SEAT", None)
            else:
                env["LJOS_SEAT"] = server_seat
            server = root / "ljos-mcp"
            if self.replace_binary:
                shutil.copyfile(BINARY, server)
                server.chmod(0o700)
            marker = root / "forwarded"
            with subprocess.Popen([str(server) if self.replace_binary else BINARY],
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, env=env, text=True,
                                  start_new_session=True) as proc:
                def send(value):
                    proc.stdin.write(json.dumps(value) + "\n")
                    proc.stdin.flush()

                def read():
                    with selectors.DefaultSelector() as selector:
                        selector.register(proc.stdout, selectors.EVENT_READ)
                        self.assertTrue(selector.select(10), "MCP response timed out")
                    line = proc.stdout.readline()
                    self.assertTrue(line, "MCP server closed without an answer")
                    return json.loads(line)

                def response(identifier):
                    while True:
                        value = read()
                        if value.get("id") == identifier:
                            return value
                        self.assertNotIn("id", value, value)

                try:
                    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                        "protocolVersion": self.protocol,
                        "clientInfo": {"name": "consent-wire-test", "version": "1"},
                        "capabilities": capabilities if capabilities is not None
                        else {"elicitation": {"form": {}}},
                    }})
                    self.assertIn("result", response(1))
                    if self.replace_binary:
                        replacement = root / "replacement"
                        replacement.write_text(
                            "#!/bin/sh\n"
                            f"printf started > {shlex.quote(str(marker))}\n"
                            f"exec {shlex.quote(BINARY)}\n"
                        )
                        replacement.chmod(0o700)
                        replacement.replace(server)
                    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
                    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
                        "name": "ljos_request_approval",
                        "arguments": arguments if arguments is not None else {"id": REQUEST_ID},
                        "_meta": {"thread_id": session},
                    }})
                    while True:
                        value = read()
                        if value.get("method") == "elicitation/create":
                            params = value["params"]
                            self.assertIn(json.dumps(COMMAND), params["message"])
                            self.assertIn(json.dumps(tmp), params["message"])
                            self.assertIn(THREAD, params["message"])
                            self.assertIn(request["scope"]["reason"], params["message"])
                            schema = params["requestedSchema"]
                            self.assertEqual(schema["required"], ["approve"])
                            self.assertIs(schema["properties"]["approve"]["default"], False)
                            self.assertIsNotNone(answer, "unexpected consent request")
                            if change:
                                rows = json.loads(path.read_text())
                                change(rows)
                                path.write_text(json.dumps(rows))
                            send({"jsonrpc": "2.0", "id": value["id"], "result": answer})
                        elif value.get("id") == 2:
                            if self.replace_binary:
                                self.assertEqual(marker.read_text(), "started")
                            return value, json.loads(path.read_text())
                        else:
                            self.assertNotIn("id", value, value)
                finally:
                    try:
                        os.killpg(proc.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                    try:
                        proc.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        os.killpg(proc.pid, signal.SIGKILL)
                        proc.wait()

    def assert_blocked(self, **kwargs):
        reply, rows = self.exchange(**kwargs)
        self.assertTrue("error" in reply or reply.get("result", {}).get("isError"), reply)
        self.assertFalse(any(row["approved"] for row in rows))

    def test_accept_records_consent(self):
        reply, rows = self.exchange(answer={"action": "accept", "content": {"approve": True}})
        self.assertNotIn("error", reply)
        self.assertFalse(reply["result"].get("isError", False))
        self.assertTrue(rows[0]["approved"])

    def test_tool_can_name_a_registered_seat(self):
        reply, rows = self.exchange(
            server_seat=None, arguments={"id": REQUEST_ID, "seat": "consent-test"},
            answer={"action": "accept", "content": {"approve": True}},
        )
        self.assertNotIn("error", reply)
        self.assertFalse(reply["result"].get("isError", False), reply)
        self.assertTrue(rows[0]["approved"])

    def test_unknown_seat_cannot_request_consent(self):
        self.assert_blocked(server_seat=None,
                            arguments={"id": REQUEST_ID, "seat": "unknown-test-seat"})

    def test_empty_legacy_capability_supports_forms(self):
        reply, rows = self.exchange(capabilities={"elicitation": {}},
                                   answer={"action": "accept", "content": {"approve": True}})
        self.assertNotIn("error", reply)
        self.assertTrue(rows[0]["approved"])

    def test_decline_cancel_false_missing_and_string_do_not_grant(self):
        for action in ("decline", "cancel", "accept"):
            for content in (None, {}, {"approve": False}, {"approve": "true"}):
                with self.subTest(action=action, content=content):
                    answer = {"action": action}
                    if content is not None:
                        answer["content"] = content
                    self.assert_blocked(answer=answer)
        for action in ("decline", "cancel"):
            with self.subTest(action=action, true_field=True):
                self.assert_blocked(answer={"action": action, "content": {"approve": True}})

    def test_no_form_capability_leaves_request_blocked(self):
        for caps in ({}, {"elicitation": {"url": {}}}):
            with self.subTest(capabilities=caps):
                self.assert_blocked(capabilities=caps)

    def test_another_conversation_cannot_request_the_grant(self):
        self.assert_blocked(session="different-test-thread")

    def test_tool_arguments_cannot_supply_consent(self):
        self.assert_blocked(arguments={"id": REQUEST_ID, "approve": True})

    def test_expired_request_cannot_open_a_dialog(self):
        self.assert_blocked(age=901)

    def test_expiry_during_confirmation_does_not_grant(self):
        def expire(rows):
            rows[0]["created"] -= 901
        self.assert_blocked(answer={"action": "accept", "content": {"approve": True}},
                            change=expire)

    def test_changed_command_during_confirmation_does_not_grant(self):
        def change(rows):
            rows[0]["scope"]["command"] += " different"
        self.assert_blocked(answer={"action": "accept", "content": {"approve": True}},
                            change=change)


class ForwardedConsentWireTest(ConsentWireTest):
    replace_binary = True


class AssociatedForwardedConsentWireTest(ForwardedConsentWireTest):
    protocol = "2026-07-28"


if __name__ == "__main__":
    unittest.main()
