#!/usr/bin/env python3
"""Compatibility shim for the original M0 bootstrap test entrypoint."""

from test_desktop_integration import *  # noqa: F401,F403
from test_desktop_integration import main


if __name__ == "__main__":
    main()
