#!/usr/bin/env python3
"""Windows live smoke: browser Network API over the desktop control pipe.

This is intentionally narrower than the SSH/WebView proxy proof. It verifies
that a current Windows desktop binary exposes the browser Network socket API and
returns the JSON shape consumed by CLI/tests/UI. With
CMUX_BROWSER_NETWORK_REQUIRE_RECORD=1 and a real test URL, it also uses the
debug WebView attach hook to require a live child-WebView navigation record.
This still does not claim remote proxy traffic completion.
"""

from __future__ import annotations

import contextlib
import http.server
import json
import os
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any


def _skip(message: str) -> int:
    print(f"SKIP: {message}")
    return 0


def _must(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def _run_json(cli: Path, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
    args = [str(cli), "rpc", method]
    if params is not None:
        args.append(json.dumps(params, separators=(",", ":")))
    proc = subprocess.run(args, capture_output=True, text=True, check=False, timeout=12)
    if proc.returncode != 0:
        merged = f"{proc.stdout}\n{proc.stderr}".strip()
        raise RuntimeError(f"cmux rpc {method} failed: {merged}")
    try:
        payload = json.loads(proc.stdout or "{}")
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"cmux rpc {method} returned invalid JSON: {proc.stdout!r}") from exc
    _must(isinstance(payload, dict), f"cmux rpc {method} should return object JSON: {payload!r}")
    return payload


def _wait_for_desktop(cli: Path, timeout_s: float = 10.0) -> dict[str, Any]:
    deadline = time.time() + timeout_s
    last_error = ""
    while time.time() < deadline:
        try:
            return _run_json(cli, "system.identify")
        except Exception as exc:  # noqa: BLE001
            last_error = str(exc)
            time.sleep(0.25)
    raise RuntimeError(f"desktop control socket did not become ready: {last_error}")


def _start_desktop(desktop: Path) -> subprocess.Popen[Any]:
    creationflags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    return subprocess.Popen(
        [str(desktop)],
        cwd=str(desktop.parents[2]),
        creationflags=creationflags,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def _validate_network_reply_shape(network: dict[str, Any], surface_id: str) -> None:
    _must(
        network.get("panelId") == surface_id or network.get("panel_id") == surface_id,
        f"browser.network.requests returned wrong panel: {network}",
    )
    _must(isinstance(network.get("requests"), list), f"network requests missing list: {network}")
    for key in ["totalCount", "returnedCount", "filteredCount"]:
        _must(isinstance(network.get(key), int), f"network reply missing {key}: {network}")

    observer = network.get("observer") or {}
    for key in [
        "capturesUrl",
        "capturesMethod",
        "capturesTiming",
        "supportsFilters",
    ]:
        _must(observer.get(key) is True, f"network observer should set {key}: {network}")
    for key in ["maxRecordsPerPanel", "bodyCaptureLimitBytes"]:
        _must(isinstance(observer.get(key), int), f"network observer missing {key}: {network}")
    _must(isinstance(observer.get("source"), str), f"network observer missing source: {network}")
    _must(
        isinstance(observer.get("proxyAttributionMode"), str),
        f"network observer missing proxy attribution mode: {network}",
    )
    _must(isinstance(observer.get("note"), str), f"network observer missing note: {network}")


def _validate_network_record_shape(record: dict[str, Any]) -> None:
    for key in ["id", "panelId", "url", "method", "source", "transport"]:
        _must(isinstance(record.get(key), str), f"network record missing string {key}: {record}")
    _must(isinstance(record.get("requestHeaders"), dict), f"record missing requestHeaders: {record}")
    _must("requestBody" in record, f"record missing requestBody key: {record}")
    _must(
        isinstance(record.get("requestBodyPreviewKind"), str),
        f"record missing requestBodyPreviewKind: {record}",
    )
    _must(isinstance(record.get("requestBodySize"), int), f"record missing requestBodySize: {record}")
    _must(
        isinstance(record.get("requestBodyTruncated"), bool),
        f"record missing requestBodyTruncated: {record}",
    )
    _must("responseStatus" in record, f"record missing responseStatus key: {record}")
    _must(isinstance(record.get("responseHeaders"), dict), f"record missing responseHeaders: {record}")
    _must("responseBody" in record, f"record missing responseBody key: {record}")
    _must(
        isinstance(record.get("responseBodyPreviewKind"), str),
        f"record missing responseBodyPreviewKind: {record}",
    )
    _must(isinstance(record.get("responseBodySize"), int), f"record missing responseBodySize: {record}")
    _must(
        isinstance(record.get("responseBodyTruncated"), bool),
        f"record missing responseBodyTruncated: {record}",
    )
    _must(isinstance(record.get("startedAtMs"), int), f"record missing startedAtMs: {record}")
    _must(
        record.get("completedAtMs") is None or isinstance(record.get("completedAtMs"), int),
        f"record completedAtMs should be null or integer: {record}",
    )
    _must(
        record.get("durationMs") is None or isinstance(record.get("durationMs"), int),
        f"record durationMs should be null or integer: {record}",
    )
    _must("proxyAttribution" in record, f"record missing proxyAttribution key: {record}")
    _must("note" in record, f"record missing backend note key: {record}")
    _must(
        record.get("note") is None or isinstance(record.get("note"), str),
        f"record note should be null or string: {record}",
    )


def _validate_proxy_record(record: dict[str, Any], marker: str) -> None:
    _must(record.get("source") == "proxy-stream-http", f"expected parsed proxy record: {record}")
    _must(record.get("proxyAttribution") == "panel", f"expected panel proxy attribution: {record}")
    _must(record.get("method") == "GET", f"expected proxied GET request: {record}")
    _must(record.get("responseStatus") == 200, f"expected HTTP 200 proxy response: {record}")
    _must(record.get("requestHeaders"), f"expected captured request headers: {record}")
    _must(record.get("responseHeaders"), f"expected captured response headers: {record}")
    _must(
        record.get("responseBodyPreviewKind") == "text",
        f"expected text response body preview: {record}",
    )
    _must(
        marker in str(record.get("responseBody") or ""),
        f"expected response body marker {marker!r}: {record}",
    )
    _must(isinstance(record.get("durationMs"), int), f"expected proxy timing duration: {record}")
    _must(
        "HTTP request/response metadata parsed" in str(record.get("note") or ""),
        f"expected parsed HTTP proxy note: {record}",
    )


class _ProxyProofHandler(http.server.BaseHTTPRequestHandler):
    marker = "cmux-proxy-proof"

    def do_GET(self) -> None:  # noqa: N802
        body = f"{self.marker}:{self.path}".encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("X-Cmux-Proxy-Proof", self.marker)
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format: str, *_args: Any) -> None:
        return


class _ThreadingHttpServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


class _AutomationProofHandler(http.server.BaseHTTPRequestHandler):
    marker = "cmux-eval-proof"

    def do_GET(self) -> None:  # noqa: N802
        if self.path.startswith("/frame.html"):
            html = """
<!doctype html>
<html>
  <body>
    <button id="frame-btn" onclick="window.top.frameClicks = (window.top.frameClicks || 0) + 1">Frame Button</button>
    <div id="frame-text">frame-ready</div>
  </body>
</html>
""".strip().encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(html)))
            self.end_headers()
            self.wfile.write(html)
            return
        if self.path.startswith("/tiny.gif"):
            body = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;"
            self.send_response(200)
            self.send_header("Content-Type", "image/gif")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        html = f"""
<!doctype html>
<html>
  <head><title>cmux-eval-proof</title></head>
  <style>
    #box {{ color: rgb(12, 34, 56); }}
    #hidden {{ display: none; }}
    #scroller {{ height: 40px; overflow: auto; }}
    #spacer {{ height: 140px; }}
  </style>
  <body>
    <section id="box" data-proof="{self.marker}">
      <h1 id="message">{self.marker}</h1>
      <label for="field">Agent Name</label>
      <input id="field" value="cmux-value" placeholder="Type name" title="name-title" data-testid="name-field">
      <img id="hero" alt="hero image" src="/tiny.gif">
      <input id="check" type="checkbox" checked>
      <select id="select"><option value="a">A</option><option value="b">B</option></select>
      <button id="button" role="button" onclick="document.querySelector('#message').textContent = 'clicked'">Submit Action</button>
      <button id="disabled" disabled>Disabled</button>
      <span class="item">one</span><span class="item">two</span>
      <span id="hidden">hidden</span>
      <iframe id="frame-a" src="/frame.html"></iframe>
      <div id="scroller"><div id="spacer"></div><div id="bottom">bottom</div></div>
      <script>window.frameClicks = 0;</script>
    </section>
  </body>
</html>
""".strip().encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(html)))
        self.end_headers()
        self.wfile.write(html)

    def log_message(self, _format: str, *_args: Any) -> None:
        return


def _start_proxy_proof_server() -> tuple[_ThreadingHttpServer, str, str, str, int]:
    server = _ThreadingHttpServer(("0.0.0.0", 0), _ProxyProofHandler)
    thread = threading.Thread(target=server.serve_forever, name="cmux-proxy-proof-http", daemon=True)
    thread.start()
    _host, port = server.server_address
    marker = f"{_ProxyProofHandler.marker}-{int(time.time() * 1000)}"
    _ProxyProofHandler.marker = marker
    url = f"http://cmux-proxy-proof.invalid:{port}/cmux-proxy-proof.txt?marker={marker}"
    return server, url, marker, "127.0.0.1", int(port)


def _start_automation_proof_server() -> tuple[_ThreadingHttpServer, str, str]:
    server = _ThreadingHttpServer(("127.0.0.1", 0), _AutomationProofHandler)
    thread = threading.Thread(
        target=server.serve_forever,
        name="cmux-automation-proof-http",
        daemon=True,
    )
    thread.start()
    _host, port = server.server_address
    marker = f"{_AutomationProofHandler.marker}-{int(time.time() * 1000)}"
    _AutomationProofHandler.marker = marker
    return server, f"http://127.0.0.1:{int(port)}/automation-proof.html", marker


def _wait_for_network_record(
    cli: Path,
    surface_id: str,
    *,
    url_token: str,
    timeout_s: float,
    source: str | None = None,
) -> dict[str, Any]:
    deadline = time.time() + timeout_s
    last: dict[str, Any] = {}
    while time.time() < deadline:
        last = _run_json(
            cli,
            "browser.network.requests",
            {"surface_id": surface_id, "urlContains": url_token, "limit": 20},
        )
        _validate_network_reply_shape(last, surface_id)
        for record in last.get("requests") or []:
            _must(isinstance(record, dict), f"network record should be an object: {record}")
            _validate_network_record_shape(record)
            if source is not None and record.get("source") != source:
                continue
            if url_token in str(record.get("url") or ""):
                return record
        time.sleep(0.25)
    raise RuntimeError(
        f"timed out waiting for browser Network record containing {url_token!r}; "
        f"last payload={last}"
    )


def _send_http_proxy_request(proxy_url: str, target_url: str, target_port: int) -> None:
    host_port = proxy_url.removeprefix("http://")
    host, port_s = host_port.rsplit(":", 1)
    request = (
        f"GET {target_url} HTTP/1.1\r\n"
        f"Host: cmux-proxy-proof.invalid:{target_port}\r\n"
        "Connection: close\r\n"
        "\r\n"
    ).encode("utf-8")
    with socket.create_connection((host, int(port_s)), timeout=5) as stream:
        stream.sendall(request)
        while stream.recv(4096):
            pass


def _wait_for_eval_value(
    cli: Path,
    surface_id: str,
    script: str,
    *,
    expected: Any,
    timeout_s: float,
) -> dict[str, Any]:
    deadline = time.time() + timeout_s
    last: dict[str, Any] = {}
    while time.time() < deadline:
        last = _run_json(cli, "browser.eval", {"surface_id": surface_id, "script": script})
        if last.get("value") == expected:
            return last
        time.sleep(0.2)
    raise RuntimeError(
        f"timed out waiting for browser.eval {script!r} to equal {expected!r}; "
        f"last payload={last}"
    )


def _require_browser_eval_and_getters(cli: Path, surface_id: str) -> None:
    server, test_url, marker = _start_automation_proof_server()
    try:
        attached = _run_json(
            cli,
            "debug.browser.attach_webview",
            {
                "surface_id": surface_id,
                "url": test_url,
                "visible": os.environ.get("CMUX_BROWSER_AUTOMATION_ATTACH_VISIBLE", "0") != "0",
            },
        )
        _must(attached.get("attached") is True, f"debug browser WebView attach failed: {attached}")

        _wait_for_eval_value(
            cli,
            surface_id,
            "document.querySelector('#message') ? document.querySelector('#message').textContent : ''",
            expected=marker,
            timeout_s=float(os.environ.get("CMUX_BROWSER_AUTOMATION_TIMEOUT_S", "20")),
        )
        waited = _run_json(cli, "browser.wait", {"surface_id": surface_id, "selector": "#message", "timeout_ms": 3000})
        _must(waited.get("value") is True, f"browser.wait selector failed: {waited}")
        eval_object = _run_json(
            cli,
            "browser.eval",
            {
                "surface_id": surface_id,
                "script": "({title: document.title, marker: document.querySelector('#message').textContent})",
            },
        )
        _must(eval_object.get("value", {}).get("marker") == marker, f"browser.eval object failed: {eval_object}")

        title = _run_json(cli, "browser.get.title", {"surface_id": surface_id})
        _must(title.get("title") == "cmux-eval-proof", f"browser.get.title failed: {title}")
        text = _run_json(cli, "browser.get.text", {"surface_id": surface_id, "selector": "#message"})
        _must(text.get("value") == marker and text.get("text") == marker, f"browser.get.text failed: {text}")
        html_payload = _run_json(cli, "browser.get.html", {"surface_id": surface_id, "selector": "#box"})
        _must(marker in str(html_payload.get("html") or ""), f"browser.get.html failed: {html_payload}")
        value_payload = _run_json(cli, "browser.get.value", {"surface_id": surface_id, "selector": "#field"})
        _must(value_payload.get("value") == "cmux-value", f"browser.get.value failed: {value_payload}")
        attr = _run_json(
            cli,
            "browser.get.attr",
            {"surface_id": surface_id, "selector": "#box", "attribute": "data-proof"},
        )
        _must(attr.get("value") == marker, f"browser.get.attr failed: {attr}")
        count = _run_json(cli, "browser.get.count", {"surface_id": surface_id, "selector": ".item"})
        _must(count.get("value") == 2 and count.get("count") == 2, f"browser.get.count failed: {count}")
        box = _run_json(cli, "browser.get.box", {"surface_id": surface_id, "selector": "#box"})
        _must(float(box.get("value", {}).get("width") or 0) > 0, f"browser.get.box failed: {box}")
        color = _run_json(
            cli,
            "browser.get.styles",
            {"surface_id": surface_id, "selector": "#box", "property": "color"},
        )
        _must("rgb" in str(color.get("value") or ""), f"browser.get.styles property failed: {color}")
        visible = _run_json(cli, "browser.is.visible", {"surface_id": surface_id, "selector": "#box"})
        _must(visible.get("value") is True, f"browser.is.visible failed: {visible}")
        hidden = _run_json(cli, "browser.is.visible", {"surface_id": surface_id, "selector": "#hidden"})
        _must(hidden.get("value") is False, f"browser.is.visible false failed: {hidden}")
        enabled = _run_json(cli, "browser.is.enabled", {"surface_id": surface_id, "selector": "#field"})
        _must(enabled.get("value") is True, f"browser.is.enabled true failed: {enabled}")
        disabled = _run_json(cli, "browser.is.enabled", {"surface_id": surface_id, "selector": "#disabled"})
        _must(disabled.get("value") is False, f"browser.is.enabled false failed: {disabled}")
        checked = _run_json(cli, "browser.is.checked", {"surface_id": surface_id, "selector": "#check"})
        _must(checked.get("value") is True, f"browser.is.checked failed: {checked}")
        _run_json(cli, "browser.uncheck", {"surface_id": surface_id, "selector": "#check"})
        unchecked = _run_json(cli, "browser.is.checked", {"surface_id": surface_id, "selector": "#check"})
        _must(unchecked.get("value") is False, f"browser.uncheck failed: {unchecked}")
        _run_json(cli, "browser.fill", {"surface_id": surface_id, "selector": "#field", "text": "filled"})
        _run_json(cli, "browser.type", {"surface_id": surface_id, "selector": "#field", "text": "-typed"})
        typed = _run_json(cli, "browser.get.value", {"surface_id": surface_id, "selector": "#field"})
        _must(typed.get("value") == "filled-typed", f"browser.fill/type failed: {typed}")
        selected = _run_json(cli, "browser.select", {"surface_id": surface_id, "selector": "#select", "value": "b"})
        _must(selected.get("value") == "b", f"browser.select failed: {selected}")
        _run_json(cli, "browser.click", {"surface_id": surface_id, "selector": "#button"})
        _wait_for_eval_value(
            cli,
            surface_id,
            "document.querySelector('#message').textContent",
            expected="clicked",
            timeout_s=5.0,
        )
        _run_json(cli, "browser.scroll", {"surface_id": surface_id, "selector": "#scroller", "dy": 80})
        scrolled = _run_json(
            cli,
            "browser.eval",
            {"surface_id": surface_id, "script": "document.querySelector('#scroller').scrollTop > 0"},
        )
        _must(scrolled.get("value") is True, f"browser.scroll failed: {scrolled}")
        _run_json(cli, "browser.addscript", {"surface_id": surface_id, "script": "document.body.style.background = 'rgb(0, 255, 0)'; true;"})
        shot = _run_json(cli, "browser.screenshot", {"surface_id": surface_id})
        _must(len(str(shot.get("png_base64") or "")) > 100, f"browser.screenshot failed: {shot}")
        snapshot = _run_json(cli, "browser.snapshot", {"surface_id": surface_id})
        _must("cmux-eval-proof" in str(snapshot.get("snapshot") or ""), f"browser.snapshot failed: {snapshot}")
        _must(isinstance(snapshot.get("refs"), dict) and snapshot.get("refs"), f"browser.snapshot refs failed: {snapshot}")
        highlighted = _run_json(cli, "browser.highlight", {"surface_id": surface_id, "selector": "#button"})
        _must(highlighted.get("highlighted") is True, f"browser.highlight failed: {highlighted}")
        init_added = _run_json(
            cli,
            "browser.addinitscript",
            {"surface_id": surface_id, "script": "window.__cmuxInitMarker = 'init-ok';"},
        )
        _must(init_added.get("added") is True, f"browser.addinitscript failed: {init_added}")
        _wait_for_eval_value(
            cli,
            surface_id,
            "window.__cmuxInitMarker || ''",
            expected="init-ok",
            timeout_s=5.0,
        )
        find_role = _run_json(
            cli,
            "browser.find.role",
            {"surface_id": surface_id, "role": "button", "name": "submit"},
        )
        role_ref = str(find_role.get("element_ref") or find_role.get("elementRef") or "")
        _must(role_ref.startswith("@e"), f"browser.find.role failed: {find_role}")
        _run_json(cli, "browser.click", {"surface_id": surface_id, "selector": role_ref})
        _wait_for_eval_value(
            cli,
            surface_id,
            "document.querySelector('#message').textContent",
            expected="clicked",
            timeout_s=5.0,
        )
        find_cases = [
            ("browser.find.text", {"text": "two"}),
            ("browser.find.label", {"label": "Agent Name"}),
            ("browser.find.placeholder", {"placeholder": "Type name"}),
            ("browser.find.alt", {"alt": "hero image"}),
            ("browser.find.title", {"title": "name-title"}),
            ("browser.find.testid", {"testid": "name-field"}),
            ("browser.find.first", {"selector": ".item"}),
            ("browser.find.last", {"selector": ".item"}),
            ("browser.find.nth", {"selector": ".item", "index": 1}),
        ]
        for method, extra in find_cases:
            params = {"surface_id": surface_id}
            params.update(extra)
            found = _run_json(cli, method, params)
            ref = str(found.get("element_ref") or found.get("elementRef") or "")
            _must(ref.startswith("@e"), f"{method} failed: {found}")
        addscript = _run_json(cli, "browser.addscript", {"surface_id": surface_id, "script": "1 + 2"})
        _must(addscript.get("value") == 3, f"browser.addscript failed: {addscript}")
        _run_json(cli, "browser.addstyle", {"surface_id": surface_id, "css": "#box { color: rgb(0, 128, 0); }"})
        style_after = _run_json(
            cli,
            "browser.get.styles",
            {"surface_id": surface_id, "selector": "#box", "property": "color"},
        )
        _must("0, 128, 0" in str(style_after.get("value") or ""), f"browser.addstyle failed: {style_after}")
        _run_json(cli, "browser.cookies.set", {"surface_id": surface_id, "name": "cmux_cookie", "value": "cookie_value"})
        cookies = _run_json(cli, "browser.cookies.get", {"surface_id": surface_id, "name": "cmux_cookie"})
        _must(
            any(row.get("name") == "cmux_cookie" and row.get("value") == "cookie_value" for row in cookies.get("cookies") or []),
            f"browser.cookies get/set failed: {cookies}",
        )
        _run_json(cli, "browser.cookies.clear", {"surface_id": surface_id, "name": "cmux_cookie"})
        cleared_cookies = _run_json(cli, "browser.cookies.get", {"surface_id": surface_id, "name": "cmux_cookie"})
        _must(not (cleared_cookies.get("cookies") or []), f"browser.cookies.clear failed: {cleared_cookies}")
        _run_json(cli, "browser.storage.set", {"surface_id": surface_id, "type": "local", "key": "alpha", "value": "one"})
        storage = _run_json(cli, "browser.storage.get", {"surface_id": surface_id, "type": "local", "key": "alpha"})
        _must(storage.get("value") == "one", f"browser.storage get/set failed: {storage}")
        _run_json(cli, "browser.storage.clear", {"surface_id": surface_id, "type": "local", "key": "alpha"})
        storage_cleared = _run_json(cli, "browser.storage.get", {"surface_id": surface_id, "type": "local", "key": "alpha"})
        _must(storage_cleared.get("value") is None, f"browser.storage.clear failed: {storage_cleared}")
        state_file = tempfile.NamedTemporaryFile(delete=False, prefix="cmux-browser-state-", suffix=".json").name
        try:
            _run_json(cli, "browser.storage.set", {"surface_id": surface_id, "type": "local", "key": "persist", "value": "yes"})
            saved_state = _run_json(cli, "browser.state.save", {"surface_id": surface_id, "path": state_file})
            _must(saved_state.get("saved") is True, f"browser.state.save failed: {saved_state}")
            _run_json(cli, "browser.storage.set", {"surface_id": surface_id, "type": "local", "key": "persist", "value": "no"})
            loaded_state = _run_json(cli, "browser.state.load", {"surface_id": surface_id, "path": state_file})
            _must(loaded_state.get("loaded") is True, f"browser.state.load failed: {loaded_state}")
            restored = _run_json(cli, "browser.storage.get", {"surface_id": surface_id, "type": "local", "key": "persist"})
            _must(restored.get("value") == "yes", f"browser.state.load did not restore storage: {restored}")
        finally:
            with contextlib.suppress(Exception):
                os.unlink(state_file)
        tabs_before = _run_json(cli, "browser.tab.list", {"surface_id": surface_id})
        before_count = len(tabs_before.get("tabs") or [])
        tab_new = _run_json(cli, "browser.tab.new", {"surface_id": surface_id, "url": "about:blank"})
        tab_surface_id = str(tab_new.get("surface_id") or tab_new.get("panel_id") or "")
        _must(tab_surface_id, f"browser.tab.new failed: {tab_new}")
        tabs_after = _run_json(cli, "browser.tab.list", {"surface_id": tab_surface_id})
        tab_ids = {str(row.get("id") or row.get("surface_id") or "") for row in tabs_after.get("tabs") or []}
        _must(
            tab_surface_id in tab_ids and len(tab_ids) >= before_count + 1,
            f"browser.tab.list failed after tab.new: {tabs_after}",
        )
        _run_json(cli, "browser.tab.switch", {"surface_id": tab_surface_id, "target_surface_id": surface_id})
        _run_json(cli, "browser.tab.close", {"surface_id": surface_id, "target_surface_id": tab_surface_id})
        _run_json(cli, "browser.frame.select", {"surface_id": surface_id, "selector": "#frame-a"})
        frame_wait = _run_json(
            cli,
            "browser.wait",
            {"surface_id": surface_id, "selector": "#frame-text", "timeout_ms": 5000},
        )
        _must(frame_wait.get("value") is True, f"browser.frame.select did not affect wait: {frame_wait}")
        frame_text = _run_json(cli, "browser.get.text", {"surface_id": surface_id, "selector": "#frame-text"})
        _must(frame_text.get("value") == "frame-ready", f"browser.frame.select did not affect getter: {frame_text}")
        _run_json(cli, "browser.click", {"surface_id": surface_id, "selector": "#frame-btn"})
        _run_json(cli, "browser.frame.main", {"surface_id": surface_id})
        frame_clicks = _run_json(cli, "browser.eval", {"surface_id": surface_id, "script": "window.frameClicks || 0"})
        _must(int(frame_clicks.get("value") or 0) >= 1, f"browser.frame.main did not restore main frame: {frame_clicks}")
        _run_json(
            cli,
            "browser.addscript",
            {
                "surface_id": surface_id,
                "script": "confirm('confirm-message'); prompt('prompt-message', 'prompt-default'); alert('alert-message'); true;",
            },
        )
        accepted = _run_json(cli, "browser.dialog.accept", {"surface_id": surface_id, "text": "agent-text"})
        dismissed = _run_json(cli, "browser.dialog.dismiss", {"surface_id": surface_id})
        accepted_alert = _run_json(cli, "browser.dialog.accept", {"surface_id": surface_id})
        _must(accepted.get("accepted") is True and accepted.get("type") == "confirm", f"browser.dialog.accept failed: {accepted}")
        _must(dismissed.get("accepted") is False and dismissed.get("type") == "prompt", f"browser.dialog.dismiss failed: {dismissed}")
        _must(accepted_alert.get("accepted") is True and accepted_alert.get("type") == "alert", f"browser.dialog.accept alert failed: {accepted_alert}")
        download_file = tempfile.NamedTemporaryFile(delete=False, prefix="cmux-browser-download-", suffix=".txt").name
        os.unlink(download_file)
        try:
            def _write_download() -> None:
                time.sleep(0.3)
                Path(download_file).write_text("downloaded", encoding="utf-8")

            thread = threading.Thread(target=_write_download, daemon=True)
            thread.start()
            downloaded = _run_json(
                cli,
                "browser.download.wait",
                {"surface_id": surface_id, "path": download_file, "timeout_ms": 5000},
            )
            _must(downloaded.get("downloaded") is True, f"browser.download.wait failed: {downloaded}")
        finally:
            with contextlib.suppress(Exception):
                os.unlink(download_file)
        _run_json(cli, "browser.console.list", {"surface_id": surface_id})
        _run_json(
            cli,
            "browser.addscript",
            {
                "surface_id": surface_id,
                "script": "console.log('cmux-console-entry'); setTimeout(() => { throw new Error('cmux-boom'); }, 0); true;",
            },
        )
        deadline = time.time() + 8.0
        console_payload: dict[str, Any] = {}
        errors_payload: dict[str, Any] = {}
        while time.time() < deadline:
            console_payload = _run_json(cli, "browser.console.list", {"surface_id": surface_id})
            errors_payload = _run_json(cli, "browser.errors.list", {"surface_id": surface_id})
            if int(console_payload.get("count") or 0) >= 1 and int(errors_payload.get("count") or 0) >= 1:
                break
            time.sleep(0.2)
        _must(
            any("cmux-console-entry" in str(row.get("text") or "") for row in console_payload.get("entries") or []),
            f"browser.console.list failed: {console_payload}",
        )
        _must(
            any("cmux-boom" in str(row.get("message") or "") for row in errors_payload.get("errors") or []),
            f"browser.errors.list failed: {errors_payload}",
        )
        cleared_console = _run_json(cli, "browser.console.clear", {"surface_id": surface_id})
        _must(int(cleared_console.get("count") or 0) == 0, f"browser.console.clear failed: {cleared_console}")
    finally:
        with contextlib.suppress(Exception):
            server.shutdown()
        with contextlib.suppress(Exception):
            server.server_close()


def main() -> int:
    if os.name != "nt":
        return _skip("Windows named-pipe live smoke only")

    repo = Path(__file__).resolve().parents[1]
    desktop = repo / "target" / "debug" / "cmux-desktop.exe"
    cli = repo / "target" / "debug" / "cmux.exe"
    if not desktop.is_file() or not cli.is_file():
        return _skip("build target/debug/cmux-desktop.exe and target/debug/cmux.exe first")

    started: subprocess.Popen[Any] | None = None
    surface_id = ""
    proof_server: _ThreadingHttpServer | None = None
    try:
        try:
            identify = _run_json(cli, "system.identify")
        except Exception:
            started = _start_desktop(desktop)
            identify = _wait_for_desktop(cli)

        _must(identify.get("app") == "cmux", f"unexpected desktop identity: {identify}")
        capabilities = _run_json(cli, "system.capabilities")
        methods = set(capabilities.get("methods") or [])
        for method in [
            "browser.network.requests",
            "browser.network.clear",
            "browser.open_split",
            "browser.eval",
            "browser.get.title",
            "browser.get.text",
            "browser.get.value",
            "browser.is.visible",
            "browser.screenshot",
            "browser.find.role",
            "browser.find.testid",
            "browser.frame.select",
            "browser.dialog.accept",
            "browser.download.wait",
            "browser.cookies.get",
            "browser.storage.get",
            "browser.tab.list",
            "browser.console.list",
            "browser.errors.list",
            "browser.state.save",
            "browser.state.load",
            "browser.highlight",
            "browser.addinitscript",
            "browser.addscript",
            "browser.addstyle",
            "debug.browser.start_direct_proxy",
            "debug.browser.attach_webview",
        ]:
            _must(method in methods, f"missing live method {method}: {capabilities}")

        opened = _run_json(cli, "browser.open_split", {"url": "about:blank"})
        surface_id = str(opened.get("surface_id") or opened.get("panel_id") or "")
        _must(surface_id, f"browser.open_split did not return a surface id: {opened}")

        cleared = _run_json(cli, "browser.network.clear", {"surface_id": surface_id})
        _must(
            cleared.get("panelId") == surface_id or cleared.get("panel_id") == surface_id,
            f"browser.network.clear returned wrong panel: {cleared}",
        )
        _must(
            isinstance(cleared.get("clearedCount", cleared.get("cleared_count")), int),
            f"browser.network.clear should report cleared count: {cleared}",
        )

        network = _run_json(
            cli,
            "browser.network.requests",
            {"surface_id": surface_id, "limit": 5, "urlContains": "about:"},
        )
        _validate_network_reply_shape(network, surface_id)

        if os.environ.get("CMUX_BROWSER_AUTOMATION_REQUIRE_EVAL") == "1":
            _require_browser_eval_and_getters(cli, surface_id)
            print(
                "PASS: Windows desktop control pipe executed browser.eval "
                "plus JS-backed automation, screenshot, init scripts, locators, frames/dialogs/downloads, cookies/storage, scripts/styles, tabs, console/errors, and state save/load against a live WebView"
            )
            return 0

        require_proxy_record = os.environ.get("CMUX_BROWSER_NETWORK_REQUIRE_PROXY_RECORD") == "1"
        require_proxy_broker_record = (
            os.environ.get("CMUX_BROWSER_NETWORK_REQUIRE_PROXY_BROKER_RECORD") == "1"
        )
        proxy_marker = ""
        proxy_target_host = ""
        proxy_target_port = 0
        if require_proxy_record or require_proxy_broker_record:
            (
                proof_server,
                test_url,
                proxy_marker,
                proxy_target_host,
                proxy_target_port,
            ) = _start_proxy_proof_server()
        else:
            test_url = os.environ.get("CMUX_BROWSER_NETWORK_TEST_URL", "about:blank").strip() or "about:blank"
        initial_navigation_url = "about:blank" if (require_proxy_record or require_proxy_broker_record) else test_url
        navigated = _run_json(
            cli,
            "browser.navigate",
            {"surface_id": surface_id, "url": initial_navigation_url},
        )
        _must(
            navigated.get("surface_id") == surface_id or navigated.get("panel_id") == surface_id,
            f"browser.navigate returned wrong surface: {navigated}",
        )

        if require_proxy_record or require_proxy_broker_record:
            _run_json(cli, "browser.network.clear", {"surface_id": surface_id})
            direct_proxy = _run_json(
                cli,
                "debug.browser.start_direct_proxy",
                {
                    "surface_id": surface_id,
                    "targetHost": proxy_target_host,
                    "targetPort": proxy_target_port,
                },
            )
            proxy_url = str(direct_proxy.get("proxy_url") or direct_proxy.get("proxyUrl") or "")
            _must(proxy_url.startswith("http://"), f"direct proxy did not return HTTP proxy URL: {direct_proxy}")
            if require_proxy_broker_record:
                _send_http_proxy_request(proxy_url, test_url, proxy_target_port)
                record = _wait_for_network_record(
                    cli,
                    surface_id,
                    url_token=proxy_marker,
                    timeout_s=float(os.environ.get("CMUX_BROWSER_NETWORK_TIMEOUT_S", "20")),
                    source="proxy-stream-http",
                )
                _validate_proxy_record(record, proxy_marker)
                print(
                    "PASS: Windows desktop control pipe emitted a parsed broker proxy Network record "
                    f"for {record.get('url')}"
                )
                return 0

            attached = _run_json(
                cli,
                "debug.browser.attach_webview",
                {
                    "surface_id": surface_id,
                    "url": test_url,
                    "proxyUrl": proxy_url,
                    "visible": os.environ.get("CMUX_BROWSER_NETWORK_ATTACH_VISIBLE", "1") != "0",
                },
            )
            _must(attached.get("attached") is True, f"debug browser WebView attach failed: {attached}")
            _must(
                attached.get("proxyApplied") is True or attached.get("proxy_applied") is True,
                f"debug browser WebView attach did not apply proxy config: {attached}",
            )
            record = _wait_for_network_record(
                cli,
                surface_id,
                url_token=proxy_marker,
                timeout_s=float(os.environ.get("CMUX_BROWSER_NETWORK_TIMEOUT_S", "20")),
                source="proxy-stream-http",
            )
            _validate_proxy_record(record, proxy_marker)
            print(
                "PASS: Windows desktop control pipe emitted a live parsed proxy Network record "
                f"for {record.get('url')}"
            )
            return 0

        if os.environ.get("CMUX_BROWSER_NETWORK_REQUIRE_RECORD") == "1":
            if test_url == "about:blank":
                raise RuntimeError(
                    "CMUX_BROWSER_NETWORK_REQUIRE_RECORD=1 requires "
                    "CMUX_BROWSER_NETWORK_TEST_URL to be a real URL"
                )
            _run_json(cli, "browser.network.clear", {"surface_id": surface_id})
            attached = _run_json(
                cli,
                "debug.browser.attach_webview",
                {
                    "surface_id": surface_id,
                    "url": test_url,
                    "visible": os.environ.get("CMUX_BROWSER_NETWORK_ATTACH_VISIBLE", "1") != "0",
                },
            )
            _must(attached.get("attached") is True, f"debug browser WebView attach failed: {attached}")
            _must(
                attached.get("proxyApplied") is False or attached.get("proxy_applied") is False,
                f"plain WebView attach should not report proxy config: {attached}",
            )
            record = _wait_for_network_record(
                cli,
                surface_id,
                url_token=os.environ.get("CMUX_BROWSER_NETWORK_URL_TOKEN", test_url),
                timeout_s=float(os.environ.get("CMUX_BROWSER_NETWORK_TIMEOUT_S", "20")),
            )
            print(
                "PASS: Windows desktop control pipe emitted a live browser Network record "
                f"for {record.get('url')}"
            )
            return 0

        print(
            "PASS: Windows desktop control pipe exposes browser Network requests/clear "
            "and returns the expected live JSON observer shape"
        )
        return 0
    finally:
        if surface_id:
            try:
                _run_json(cli, "surface.close", {"surface_id": surface_id})
            except Exception:
                pass
        if proof_server is not None:
            with contextlib.suppress(Exception):
                proof_server.shutdown()
            with contextlib.suppress(Exception):
                proof_server.server_close()
        if started is not None:
            started.terminate()
            try:
                started.wait(timeout=5)
            except subprocess.TimeoutExpired:
                started.kill()


if __name__ == "__main__":
    raise SystemExit(main())
