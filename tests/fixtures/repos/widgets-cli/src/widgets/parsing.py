"""Command parsing helpers — currently unused (dispatch is in cli.py)."""
from __future__ import annotations


KNOWN_COMMANDS = {"add", "remove", "list", "report"}


def parse_command(argv: list[str]) -> tuple[str, list[str]]:
    """Parse argv into (command, remaining_args)."""
    if not argv:
        return "", []
    return argv[0], argv[1:]
