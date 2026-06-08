# Diagnostic: unbounded_cycle

A workflow containing a cycle (task A -> task B -> task A) where none of the
tasks in the cycle have a `max_iterations` cap set should be rejected by the
compiler with a `CompilerError` indicating the cycle is unbounded.

Every cycle in the task graph must have at least one task with
`repeat_at_most(N)` (compiled to `max_iterations: N`) set. This ensures the
workflow cannot loop infinitely at the structural level.

Example:
```python
a = wf.task("a", command("echo a"))
b = wf.task("b", command("echo b"))
a.then(b)
b.then(a)  # back-edge with no max_iterations -> CompilerError
```

Expected compiler output: `CompilerError: unbounded cycle detected involving tasks: a, b`
