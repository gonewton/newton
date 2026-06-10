"""CLI smoke tests — behaviour lock."""
import sys
import pytest
from unittest.mock import patch
from widgets.inventory import _STORE
from widgets import cli


@pytest.fixture(autouse=True)
def clear_store():
    _STORE.clear()
    yield
    _STORE.clear()


def test_cli_add(capsys):
    with patch.object(sys, "argv", ["widgets", "add", "foo"]):
        cli.main()


def test_cli_list(capsys):
    _STORE.append("bar")
    with patch.object(sys, "argv", ["widgets", "list"]):
        cli.main()
    out = capsys.readouterr().out
    assert "bar" in out


def test_cli_unknown_exits():
    with patch.object(sys, "argv", ["widgets", "xyzzy"]):
        with pytest.raises(SystemExit):
            cli.main()
