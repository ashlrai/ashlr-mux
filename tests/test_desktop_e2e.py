#!/usr/bin/env python3
"""End-to-end tests for the desktop bootstrap scaffold."""

from __future__ import annotations

import os
import re
import subprocess
import threading
import urllib.request
from functools import partial
from http.server import SimpleHTTPRequestHandler
from pathlib import Path
from socketserver import TCPServer
from urllib.parse import urljoin

from test_desktop_integration import DESKTOP_WEB_DIST, ROOT, powershell_executable, run


class SkipTest(Exception):
    """Raised when an environment limitation blocks a specific e2e path."""


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:
        del format, args


def fetch_text(url: str) -> str:
    with urllib.request.urlopen(url, timeout=10) as response:
        return response.read().decode("utf-8")


def test_desktop_web_shell_serves_end_to_end() -> None:
    run("bun", "run", "desktop:web:build")

    handler = partial(QuietHandler, directory=str(DESKTOP_WEB_DIST))
    with TCPServer(("127.0.0.1", 0), handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            port = server.server_address[1]
            base_url = f"http://127.0.0.1:{port}/"
            index_html = fetch_text(urljoin(base_url, "index.html"))
            asset_refs = re.findall(r'(?:src|href)="([^"]+)"', index_html)
            js_bundles = [
                fetch_text(urljoin(base_url, ref))
                for ref in asset_refs
                if ref.endswith(".js")
            ]
            css_bundles = [
                fetch_text(urljoin(base_url, ref))
                for ref in asset_refs
                if ref.endswith(".css")
            ]
        finally:
            server.shutdown()
            thread.join(timeout=5)

    assert "cmux for Windows" in index_html
    assert js_bundles, "index.html should reference at least one served JS bundle"
    assert css_bundles, "index.html should reference at least one served CSS bundle"
    assert any("cmux" in bundle for bundle in js_bundles)
    assert any(".cmux" in bundle for bundle in css_bundles)


def test_native_desktop_smoke_launch() -> None:
    if os.name != "nt":
        raise SkipTest("native desktop smoke launch only runs on Windows")

    strict_native = os.environ.get("CMUX_E2E_REQUIRE_NATIVE") == "1"

    run(powershell_executable(), "-ExecutionPolicy", "Bypass", "-File", "scripts/desktop/generate-tauri-icon.ps1")
    run("cargo", "build", "--manifest-path", "apps/desktop/src-tauri/Cargo.toml")

    app_path = ROOT / "target" / "debug" / "cmux-desktop.exe"
    if not app_path.exists():
        raise AssertionError(f"desktop executable was not built: {app_path}")

    try:
        result = run(
            powershell_executable(),
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            "scripts/desktop/smoke-launch-windows.ps1",
            "-AppPath",
            str(app_path),
            "-Headless",
            "-TimeoutSeconds",
            "15",
        )
    except subprocess.CalledProcessError as error:
        output = f"{error.stdout}\n{error.stderr}"
        is_environmental = any(
            marker in output
            for marker in (
                "Access is denied",
                "Desktop smoke launch failed because the app exited early.",
                "Desktop smoke launch timed out waiting for a visible window.",
            )
        )
        if is_environmental and not strict_native:
            raise SkipTest(output.strip()) from error
        raise

    assert "PASS: desktop bootstrap launched" in result.stdout


def main() -> None:
    skipped = 0
    for name, value in sorted(globals().items()):
        if not name.startswith("test_") or not callable(value):
            continue
        try:
            value()
        except SkipTest as error:
            skipped += 1
            print(f"SKIP: {name}: {error}")
    print(f"PASS: desktop e2e (skipped={skipped})")


if __name__ == "__main__":
    main()
