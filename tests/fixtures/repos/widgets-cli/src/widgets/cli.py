"""Entry point: parse args and dispatch to widget commands."""
import sys
from widgets.inventory import process
from widgets.formatting import print_table


def main() -> None:
    args = sys.argv[1:]
    if not args:
        print("Usage: widgets <command> [options]")
        sys.exit(1)

    cmd = args[0]

    # ISSUE D: substring-match dispatch — branching_discipline
    if cmd.find("add") != -1:
        name = args[1] if len(args) > 1 else ""
        process(("add", name))
    elif cmd.find("remove") != -1:
        name = args[1] if len(args) > 1 else ""
        process(("remove", name))
    elif cmd.find("list") != -1:
        items = process(("list",))
        print_table(items)
    elif cmd.find("report") != -1:
        items = process(("list",))
        from widgets.report import print_table as report_table
        report_table(items)
    else:
        print(f"Unknown command: {cmd}")
        sys.exit(1)
