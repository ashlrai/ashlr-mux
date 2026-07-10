#!/usr/bin/env python3
"""
E2E: focusing a panel clears its notification and triggers a flash.

Note: This uses the socket focus command (no assistive access needed).
"""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from cmux import cmux, cmuxError


def wait_for_notification(client: cmux, surface_id: str, is_read: bool, timeout: float = 2.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        items = client.list_notifications()
        for item in items:
            if item["surface_id"] == surface_id and item["is_read"] == is_read:
                return True
        time.sleep(0.05)
    return False


def surface_id_for_handle(client: cmux, handle: str | int) -> str:
    wanted = str(handle)
    for row in client.surface_health():
        candidates = {
            str(row.get("id") or ""),
            str(row.get("ref") or ""),
            str(row.get("surface_ref") or ""),
        }
        if wanted in candidates:
            surface_id = str(row.get("id") or "")
            if surface_id:
                return surface_id
    surfaces = client.list_surfaces()
    for entry in surfaces:
        if str(entry[0]) == wanted or str(entry[1]) == wanted:
            return entry[1]
    raise RuntimeError(f"Surface handle {handle} not found")


def ensure_two_surfaces(client: cmux) -> None:
    surfaces = client.list_surfaces()
    if len(surfaces) < 2:
        client.new_split("right")
        time.sleep(0.2)

def first_two_terminal_refs(client: cmux) -> tuple[str, str]:
    health = client.surface_health()
    terms = [
        str(h.get("ref") or h.get("surface_ref") or h.get("id") or "")
        for h in health
        if h.get("type") == "terminal"
    ]
    terms = [term for term in terms if term]
    if len(terms) < 2:
        raise RuntimeError(f"Expected >=2 terminal surfaces, got {health}")
    return terms[0], terms[1]


def main() -> int:
    try:
        with cmux() as client:
            # Socket-driven tests may run while the app isn't frontmost/key.
            # Override app focus to make notification->focus behavior deterministic.
            client.set_app_focus(True)
            ws_id = client.new_workspace()
            client.select_workspace(ws_id)
            time.sleep(0.5)
            ensure_two_surfaces(client)
            term_a, term_b = first_two_terminal_refs(client)
            client.focus_surface(term_a)

            surface_id = surface_id_for_handle(client, term_b)
            client.clear_notifications()
            client.reset_flash_counts()
            initial_flash = client.flash_count(term_b)

            client.notify_surface(term_b, "Focus Test", "panel", "body")
            if not wait_for_notification(client, surface_id, is_read=False, timeout=2.0):
                print("FAIL: Notification did not appear as unread")
                return 1

            client.focus_surface(term_b)
            client.send("x")
            time.sleep(0.2)

            if not wait_for_notification(client, surface_id, is_read=True, timeout=2.0):
                print("FAIL: Notification did not become read after focus")
                return 1

            final_flash = client.flash_count(term_b)
            if final_flash <= initial_flash:
                print(f"FAIL: Flash count did not increment (before={initial_flash}, after={final_flash})")
                return 1

            try:
                client.close_workspace(ws_id)
            except Exception:
                pass
            finally:
                try:
                    client.set_app_focus(None)
                except Exception:
                    pass

            print("PASS: Focus clears notification and flashes panel")
            return 0
    except (cmuxError, RuntimeError) as exc:
        print(f"FAIL: {exc}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
