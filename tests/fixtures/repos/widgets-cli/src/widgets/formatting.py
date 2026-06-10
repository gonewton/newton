"""Formatting utilities."""
from __future__ import annotations


# ISSUE B: print_table duplicated verbatim in report.py — canonical_placement
def print_table(items: list[str]) -> None:
    """Print items as a simple ASCII table."""
    if not items:
        print("(empty)")
        return
    width = max(len(i) for i in items) + 2
    print("+" + "-" * width + "+")
    for item in items:
        print("| " + item.ljust(width - 2) + " |")
    print("+" + "-" * width + "+")
