/**
 * Tests for .out.field validation — CompilerError on unknown fields.
 */
import { describe, it, expect } from "vitest";
import { Workflow, command, agent, CompilerError, OperatorCall } from "../src/index.js";

describe("out.field validation", () => {
  it("raises CompilerError for unknown field on CommandOperator", () => {
    const wf = new Workflow("test", { defaultEngine: "codex" });
    const t = wf.task("cmd", command({ cmd: "echo hi", captureStdout: true }));
    expect(() => t.out.badfield).toThrow(CompilerError);
    expect(() => t.out.badfield).toThrow(/no output field 'badfield'/);
  });

  it("allows all known CommandOperator fields without error", () => {
    const wf = new Workflow("test", { defaultEngine: "codex" });
    const t = wf.task("cmd", command({ cmd: "echo" }));
    for (const field of ["stdout", "stderr", "exit_code", "success", "duration_ms"]) {
      const ref = (t.out as unknown as Record<string, { rhaiExpr(): string }>)[field];
      expect(ref.rhaiExpr()).toBe(`tasks.cmd.output.${field}`);
    }
  });

  it("allows any field for operators with no known schema", () => {
    const wf = new Workflow("test", { defaultEngine: "codex" });
    const t = wf.task("custom", new OperatorCall("UnknownOperator", {}));
    const ref = t.out.anyField;
    expect(ref.rhaiExpr()).toBe("tasks.custom.output.anyField");
  });

  it("error message includes known field list", () => {
    const wf = new Workflow("test", { defaultEngine: "codex" });
    const t = wf.task("a", agent());
    expect(() => t.out.nosuchfield).toThrow(/stdout/);
  });

  it("raises CompilerError (not a plain Error) for unknown field", () => {
    const wf = new Workflow("test", { defaultEngine: "codex" });
    const t = wf.task("cmd", command({ cmd: "ls" }));
    let caught: unknown;
    try {
      void t.out.badfield;
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(CompilerError);
    expect((caught as CompilerError).name).toBe("CompilerError");
  });
});
