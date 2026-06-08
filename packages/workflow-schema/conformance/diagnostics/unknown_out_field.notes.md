# Diagnostic: unknown_out_field

A reference `t.out.nonexistent` where `nonexistent` is not a known field in
the operator's output schema should be flagged by the compiler.

When an operator has a known, typed output schema (e.g. CommandOperator
outputs `{stdout, stderr, exit_code}`), accessing `t.out.nonexistent` is a
compile-time error because the field does not exist in the schema.

Example:
```python
cmd_task = wf.task("my_cmd", command("echo hello", capture_stdout=True))
# Access a field that doesn't exist in CommandOperator output schema
wf.task("next", agent(prompt=cmd_task.out.nonexistent_field))
```

Expected compiler output:
`CompilerError: task 'my_cmd' (CommandOperator) has no output field 'nonexistent_field'; known fields: stdout, stderr, exit_code`
