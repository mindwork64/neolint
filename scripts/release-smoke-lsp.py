#!/usr/bin/env python3
"""Exercise the packaged LSP executable using a real JSON-RPC session."""

from __future__ import annotations

import argparse
import json
import queue
import subprocess
import threading
from pathlib import Path
from typing import Any


def read_messages(stream: Any, messages: queue.Queue[Any]) -> None:
    try:
        while True:
            headers: dict[str, str] = {}
            while True:
                line = stream.readline()
                if not line:
                    messages.put(EOFError("language server closed stdout"))
                    return
                if line in (b"\r\n", b"\n"):
                    break
                key, separator, value = line.decode("ascii").partition(":")
                if not separator:
                    raise RuntimeError(f"invalid LSP header: {line!r}")
                headers[key.strip().lower()] = value.strip()
            length = int(headers["content-length"])
            payload = stream.read(length)
            if len(payload) != length:
                raise EOFError("truncated LSP message")
            messages.put(json.loads(payload))
    except BaseException as error:
        messages.put(error)


def receive(messages: queue.Queue[Any], expected_id: int) -> dict[str, Any]:
    while True:
        item = messages.get(timeout=15)
        if isinstance(item, BaseException):
            raise item
        if item.get("id") == expected_id:
            return item
        raise RuntimeError(f"unexpected LSP message while waiting for id {expected_id}: {item!r}")


def send(stream: Any, message: dict[str, Any]) -> None:
    payload = json.dumps(message, separators=(",", ":")).encode("utf-8")
    stream.write(f"Content-Length: {len(payload)}\r\n\r\n".encode("ascii"))
    stream.write(payload)
    stream.flush()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, help="path to neolint-lsp executable")
    parser.add_argument("--version", required=True, help="expected package version")
    args = parser.parse_args()
    binary = Path(args.binary)
    if not binary.is_file():
        raise FileNotFoundError(f"language server binary not found: {binary}")

    process = subprocess.Popen(
        [str(binary.resolve())],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert process.stdin is not None
    assert process.stdout is not None
    messages: queue.Queue[Any] = queue.Queue()
    reader = threading.Thread(
        target=read_messages, args=(process.stdout, messages), daemon=True
    )
    reader.start()

    try:
        send(
            process.stdin,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": None,
                    "rootUri": None,
                    "capabilities": {
                        "textDocument": {
                            "codeAction": {
                                "codeActionLiteralSupport": {
                                    "codeActionKind": {"valueSet": ["quickfix"]}
                                }
                            }
                        }
                    },
                },
            },
        )
        initialized = receive(messages, 1)
        if "error" in initialized:
            raise RuntimeError(f"LSP initialize failed: {initialized['error']!r}")
        result = initialized.get("result", {})
        if result.get("serverInfo", {}).get("name") != "NeoLint":
            raise RuntimeError(f"unexpected LSP serverInfo: {result.get('serverInfo')!r}")
        if result["serverInfo"].get("version") != args.version:
            raise RuntimeError(
                f"unexpected LSP server version: {result['serverInfo'].get('version')!r}; "
                f"expected {args.version!r}"
            )
        capabilities = result.get("capabilities", {})
        for capability in (
            "textDocumentSync",
            "hoverProvider",
            "definitionProvider",
            "completionProvider",
            "codeActionProvider",
        ):
            if capability not in capabilities or capabilities[capability] in (None, False):
                raise RuntimeError(f"LSP initialize response omits {capability}: {initialized!r}")

        send(
            process.stdin,
            {"jsonrpc": "2.0", "method": "initialized", "params": {}},
        )
        send(
            process.stdin,
            {"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": None},
        )
        shutdown = receive(messages, 2)
        if "error" in shutdown or shutdown.get("result") is not None:
            raise RuntimeError(f"LSP shutdown failed: {shutdown!r}")

        send(process.stdin, {"jsonrpc": "2.0", "method": "exit", "params": None})
        process.stdin.close()
        return_code = process.wait(timeout=15)
        if return_code != 0:
            stderr = process.stderr.read().decode("utf-8", errors="replace") if process.stderr else ""
            raise RuntimeError(
                f"language server exited with {return_code}; stderr:\n{stderr}"
            )
    except BaseException:
        process.kill()
        process.wait()
        raise

    print("LSP release smoke: initialize, capabilities, shutdown and exit passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
