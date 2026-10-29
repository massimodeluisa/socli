#!/usr/bin/env python3
"""Use a controlling PTY to test status reports and raw-mode input together."""

from __future__ import annotations

import base64
import errno
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unittest


TIMEOUT = 10
MAX_OUTPUT = 2 * 1024 * 1024
REPORT = re.compile(rb"\x1b\]7501;([^\x1b\x07]*)(?:\x1b\\|\x07)")
BINARY = Path(sys.argv.pop(1)).resolve() if len(sys.argv) > 1 else None


def environment(config: str) -> dict[str, str]:
    prefixes = ("SOCLI_", "X_", "BLUESKY_", "DEVNULL_", "MASTODON_", "THREADS_")
    env = {key: value for key, value in os.environ.items() if not key.startswith(prefixes)}
    env.update(
        SOCLI_CONFIG_DIR=config,
        SOCLI_AGENT="0",
        SOCLI_IMAGE="off",
        TERM="xterm-256color",
        TERM_PROGRAM="rex",
    )
    return env


class Terminal:
    def __init__(self, args: list[str], env: dict[str, str]):
        self.output = bytearray()
        self.code = None
        self.closed = False
        self.queries = {}
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            try:
                fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
                os.execve(str(BINARY), [str(BINARY), *args], env)
            except BaseException:
                os._exit(127)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.poll()
        if self.code is None:
            try:
                os.kill(self.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            deadline = time.monotonic() + 1
            while self.code is None and time.monotonic() < deadline:
                time.sleep(0.01)
                self.poll()
            if self.code is None:
                os.kill(self.pid, signal.SIGKILL)
                _, status = os.waitpid(self.pid, 0)
                self.code = os.waitstatus_to_exitcode(status)
        os.close(self.fd)

    def poll(self):
        if self.code is None:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.code = os.waitstatus_to_exitcode(status)

    def send(self, value: bytes):
        view = memoryview(value)
        while view:
            view = view[os.write(self.fd, view):]

    def read(self, timeout: float = 0.05):
        if not self.closed and select.select([self.fd], [], [], timeout)[0]:
            try:
                data = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            if not data:
                self.closed = True
            self.output.extend(data)
            if len(self.output) > MAX_OUTPUT:
                raise AssertionError("terminal output exceeded 2 MiB")
            for query, reply in (
                (b"\x1b]11;?", b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
                (b"\x1b[?u", b"\x1b[?0u"),
                (b"\x1b[c", b"\x1b[?1;2c"),
            ):
                count = self.output.count(query)
                while self.queries.get(query, 0) < count:
                    self.queries[query] = self.queries.get(query, 0) + 1
                    self.send(reply)
        self.poll()

    def reports(self) -> list[dict[str, str]]:
        reports = []
        network_titles = {
            "x": "X", "bluesky": "Bluesky", "devnull": "dev/null",
            "threads": "Threads", "mastodon": "Mastodon",
        }
        for match in REPORT.finditer(self.output):
            fields = {}
            for pair in match[1].split(b":"):
                key, value = pair.split(b"=", 1)
                if not re.fullmatch(rb"[a-z]{1,16}", key) or key in fields:
                    raise AssertionError("invalid or repeated program-status key")
                if not re.fullmatch(rb"[A-Za-z0-9_.,+/=-]*", value):
                    raise AssertionError("invalid program-status value")
                fields[key] = value
            report = {key.decode("ascii"): value.decode("ascii") for key, value in fields.items()}
            state = report["state"]
            if state not in ("idle", "working", "blocked", "done", "error", "clear"):
                raise AssertionError("invalid program-status state")
            if state == "clear":
                if "id" not in report or report.get("app", "socli") != "socli":
                    raise AssertionError("invalid child clear record")
            elif report.get("app") != "socli" or "msg" not in report or "title" not in report:
                raise AssertionError("incomplete program-status record")
            if len(match[0]) > 4096:
                raise AssertionError("program-status record exceeded protocol limit")
            if state == "blocked":
                if report.get("kind") not in ("auth", "permission", "question"):
                    raise AssertionError("invalid blocked kind")
            elif "kind" in report:
                raise AssertionError("kind remained set outside a blocked report")
            if "progress" in report:
                if not report["progress"].isdigit() or not 0 <= int(report["progress"]) <= 100:
                    raise AssertionError("invalid program-status progress")
            for key, encoded_limit, decoded_limit, decoded_key in (
                ("msg", 2732, 2048, "message"), ("title", 256, 192, "title_text"),
            ):
                encoded = report.get(key)
                decoded = "" if encoded is None else base64.b64decode(encoded, validate=True).decode("utf-8")
                if encoded is not None and (len(encoded) > encoded_limit or len(decoded.encode("utf-8")) > decoded_limit):
                    raise AssertionError(f"status {key} exceeded protocol limits")
                if any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in decoded):
                    raise AssertionError(f"status {key} contains a control character")
                report[decoded_key] = decoded
            task_id = report.get("id")
            if task_id is None:
                if report["title_text"] != "SoCLI":
                    raise AssertionError("root title did not name SoCLI")
            else:
                parts = task_id.split("/")
                if len(parts) != 3 or parts[0] != "network" or parts[1] not in network_titles or parts[2] not in ("read", "post", "action", "auth"):
                    raise AssertionError("invalid network task ID")
                if "title" in report and report["title_text"] != f"{network_titles[parts[1]]} {parts[2]}":
                    raise AssertionError("child title did not match its fixed network operation")
            reports.append(report)
        return reports

    def until(self, condition, description: str):
        deadline = time.monotonic() + TIMEOUT
        while time.monotonic() < deadline:
            result = condition()
            if result:
                return result
            if self.code is not None:
                break
            self.read()
        raise AssertionError(f"{description}; exit={self.code}; tail={bytes(self.output[-2000:])!r}")

    def expect(self, state: str, kind: str | None = None, start: int = 0, task_id: str | None = None) -> int:
        def matching():
            for index, report in enumerate(self.reports()[start:], start):
                if report.get("id") == task_id and report["state"] == state and (kind is None or report.get("kind") == kind):
                    return index + 1
            return None

        return self.until(matching, f"no {state}/{kind} report")

    def raw(self):
        self.until(
            lambda: not (termios.tcgetattr(self.fd)[3] & termios.ICANON),
            "terminal did not enter raw mode",
        )

    def finish(self) -> int:
        self.until(lambda: self.code is not None, "child did not exit")
        deadline = time.monotonic() + TIMEOUT
        while not self.closed and time.monotonic() < deadline:
            self.read()
        if not self.closed:
            raise AssertionError("terminal did not close after child exit")
        return self.code


class ProgramStatusTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="socli-program-status-")
        self.addCleanup(self.directory.cleanup)
        self.env = environment(self.directory.name)

    def terminal(self, *args: str) -> Terminal:
        return Terminal(list(args), self.env)

    def assert_idle_exit(self, terminal: Terminal, code: int):
        self.assertEqual(terminal.finish(), code)
        reports = terminal.reports()
        self.assertTrue(reports)
        self.assertEqual(reports[-1]["state"], "idle")
        self.assertNotIn("id", reports[-1])
        self.assertEqual(reports[-1]["title_text"], "SoCLI")
        self.assertFalse(any(report["state"] == "error" for report in reports))

    def test_reader_quit(self):
        with self.terminal("--demo", "--images=off", "--detail=plain") as terminal:
            terminal.expect("idle")
            terminal.send(b"q")
            self.assert_idle_exit(terminal, 0)

    def test_reader_ctrl_c(self):
        with self.terminal("--demo", "--images=off", "--detail=plain") as terminal:
            terminal.expect("idle")
            terminal.send(b"\x03")
            self.assert_idle_exit(terminal, 0)

    def test_password_cancellation_hides_contents(self):
        secret = b"NeverReportThisPassword_42"
        with self.terminal(
            "login", "bluesky", "--handle", "fixture.invalid", "--service", "http://127.0.0.1:1"
        ) as terminal:
            task_id = "network/bluesky/auth"
            terminal.expect("working", task_id=task_id)
            after = terminal.expect("blocked", "auth", task_id=task_id)
            child = terminal.reports()[after - 1]
            self.assertEqual(child["title_text"], "Bluesky auth")
            self.assertEqual(child["app"], "socli")
            terminal.raw()
            terminal.send(secret + b"\x1b")
            after = terminal.expect("clear", start=after, task_id=task_id)
            terminal.expect("idle", start=after)
            self.assert_idle_exit(terminal, 1)
            for report in terminal.reports():
                self.assertNotIn(secret.decode(), report["message"])
                self.assertNotIn(secret.decode(), report["title_text"])
            self.assertNotIn(secret, terminal.output)

    def test_delete_rejection(self):
        feed = subprocess.run(
            [str(BINARY), "--demo", "--json", "--net", "bluesky", "feed"],
            env=self.env,
            capture_output=True,
            check=True,
            timeout=TIMEOUT,
        )
        posts = json.loads(feed.stdout)
        target = next(post["id"] for post in posts if post["network"] == "bluesky")
        with self.terminal("--demo", "--net", "bluesky", "delete", target) as terminal:
            after = terminal.expect("blocked", "permission")
            terminal.raw()
            terminal.send(b"n\r")
            terminal.expect("idle", start=after)
            self.assert_idle_exit(terminal, 0)
            self.assertIn(b"nothing deleted", terminal.output)

    def test_scrollback_prompt_runs_a_command(self):
        with self.terminal("--demo", "--print", "--detail=plain") as terminal:
            after = terminal.expect("idle")
            terminal.send(b"help\r")
            after = terminal.expect("working", start=after)
            terminal.expect("idle", start=after)
            terminal.send(b"quit\r")
            self.assert_idle_exit(terminal, 0)

    def test_composer_discard_question(self):
        with self.terminal("--demo", "--net", "bluesky", "post") as terminal:
            after = terminal.expect("idle")
            terminal.raw()
            terminal.send(b"Temporary draft\x1b")
            after = terminal.expect("blocked", "question", start=after)
            terminal.send(b"n")
            terminal.expect("idle", start=after)
            self.assert_idle_exit(terminal, 0)

    def test_scrollback_error_waits_for_the_next_command(self):
        with self.terminal("--demo", "--print", "--detail=plain") as terminal:
            after = terminal.expect("idle")
            terminal.send(b"not-a-command\r")
            after = terminal.expect("error", start=after)
            terminal.send(b"help\r")
            after = terminal.expect("working", start=after)
            terminal.expect("idle", start=after)
            terminal.send(b"quit\r")
            self.assertEqual(terminal.finish(), 0)
            self.assertEqual(terminal.reports()[-1]["state"], "idle")


if __name__ == "__main__":
    if BINARY is None or not BINARY.is_file():
        sys.exit("usage: python3 scripts/program-status.test.py PATH_TO_SOCLI")
    unittest.main()
