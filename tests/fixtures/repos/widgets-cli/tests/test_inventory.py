"""Behaviour lock for inventory — must stay green every cycle."""
import pytest
from widgets.inventory import process, _STORE


@pytest.fixture(autouse=True)
def clear_store():
    _STORE.clear()
    yield
    _STORE.clear()


def test_add_item():
    result = process(("add", "widget-a"))
    assert "widget-a" in result


def test_add_duplicate_raises():
    process(("add", "widget-a"))
    with pytest.raises(ValueError, match="already exists"):
        process(("add", "widget-a"))


def test_remove_item():
    process(("add", "widget-b"))
    result = process(("remove", "widget-b"))
    assert "widget-b" not in result


def test_remove_missing_raises():
    with pytest.raises(ValueError, match="not found"):
        process(("remove", "nonexistent"))


def test_list_returns_all():
    process(("add", "x"))
    process(("add", "y"))
    items = process(("list",))
    assert "x" in items
    assert "y" in items


def test_add_validates_name():
    with pytest.raises(ValueError):
        process(("add", ""))


def test_clear():
    process(("add", "z"))
    result = process(("clear",))
    assert result == []
