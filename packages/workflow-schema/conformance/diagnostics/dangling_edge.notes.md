# Diagnostic: dangling_edge

An edge (transition) pointing to a task ID that is not defined in the workflow
should be rejected by the compiler.

All task IDs referenced in `.then(target)` or `.otherwise(target)` must
correspond to a task registered with `wf.task(...)`, `wf.finish(...)`, or
`wf.fail(...)`.

Example:
```python
a = wf.task("a", command("echo a"))
b = wf.task("b", command("echo b"))
# Suppose there is no task "c" defined
a.then(b)
b.then_by_id("c")  # dangling reference -> CompilerError
```

Expected compiler output: `CompilerError: task 'b' has transition to undefined task 'c'`
