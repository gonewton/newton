/**
 * Tests for the operator builders added in spec 074 P8:
 * barrier, setContext, noop, graderCommand, reconcile, changeRequest, graderAgent.
 */
import { describe, it, expect } from "vitest";
import {
  barrier,
  setContext,
  noop,
  graderCommand,
  reconcile,
  changeRequest,
  graderAgent,
} from "../src/index.js";

describe("barrier", () => {
  it("produces the barrier operator tag with no params by default", () => {
    const call = barrier();
    expect(call.operatorType).toBe("barrier");
    expect(call.params).toEqual({});
  });

  it("includes expected task ids when provided", () => {
    const call = barrier({ expected: ["t1", "t2"] });
    expect(call.operatorType).toBe("barrier");
    expect(call.params).toEqual({ expected: ["t1", "t2"] });
  });
});

describe("setContext", () => {
  it("produces SetContextOperator with a patch object", () => {
    const call = setContext({ patch: { foo: "bar" } });
    expect(call.operatorType).toBe("SetContextOperator");
    expect(call.params).toEqual({ patch: { foo: "bar" } });
  });
});

describe("noop", () => {
  it("produces NoOpOperator with no params", () => {
    const call = noop();
    expect(call.operatorType).toBe("NoOpOperator");
    expect(call.params).toEqual({});
  });
});

describe("graderCommand", () => {
  it("produces GraderCommandOperator with required params", () => {
    const call = graderCommand({
      cmd: "./grade.sh",
      grader: "test-coverage-grader",
      scope: "module",
      scopeId: "mod-001",
    });
    expect(call.operatorType).toBe("GraderCommandOperator");
    expect(call.params).toEqual({
      cmd: "./grade.sh",
      grader: "test-coverage-grader",
      scope: "module",
      scope_id: "mod-001",
    });
  });

  it("includes optional params when provided", () => {
    const call = graderCommand({
      cmd: "./grade.sh",
      grader: "test-coverage-grader",
      scope: "module",
      scopeId: "mod-001",
      shell: "zsh",
      cwd: "sub/dir",
      timeoutSeconds: 30,
      env: { FOO: "bar" },
      state: { key: "value" },
    });
    expect(call.params).toEqual({
      cmd: "./grade.sh",
      grader: "test-coverage-grader",
      scope: "module",
      scope_id: "mod-001",
      shell: "zsh",
      cwd: "sub/dir",
      timeout_seconds: 30,
      env: { FOO: "bar" },
      state: { key: "value" },
    });
  });
});

describe("reconcile", () => {
  it("produces ReconcileOperator with required params", () => {
    const call = reconcile({
      scope: "module",
      scopeId: "mod-001",
      assessment: { overall_score: 90 },
    });
    expect(call.operatorType).toBe("ReconcileOperator");
    expect(call.params).toEqual({
      scope: "module",
      scope_id: "mod-001",
      assessment: { overall_score: 90 },
    });
  });

  it("includes optional params when provided", () => {
    const call = reconcile({
      scope: "module",
      scopeId: "mod-001",
      assessment: { overall_score: 90 },
      grader: "test-grader",
      engine: "codex",
      model: "gpt-5",
      adjudicationTimeoutSeconds: 45,
    });
    expect(call.params).toEqual({
      scope: "module",
      scope_id: "mod-001",
      assessment: { overall_score: 90 },
      grader: "test-grader",
      engine: "codex",
      model: "gpt-5",
      adjudication_timeout_seconds: 45,
    });
  });
});

describe("changeRequest", () => {
  it("produces ChangeRequestOperator with required params", () => {
    const call = changeRequest({ scope: "module", scopeId: "mod-001" });
    expect(call.operatorType).toBe("ChangeRequestOperator");
    expect(call.params).toEqual({ scope: "module", scope_id: "mod-001" });
  });

  it("includes optional params when provided", () => {
    const call = changeRequest({
      scope: "module",
      scopeId: "mod-001",
      maxFindings: 5,
      minSeverity: "high",
      engine: "codex",
      model: "gpt-5",
      synthesisTimeoutSeconds: 30,
    });
    expect(call.params).toEqual({
      scope: "module",
      scope_id: "mod-001",
      max_findings: 5,
      min_severity: "high",
      engine: "codex",
      model: "gpt-5",
      synthesis_timeout_seconds: 30,
    });
  });
});

describe("graderAgent", () => {
  it("produces GraderAgentOperator with required params", () => {
    const call = graderAgent({
      grader: "docs-quality-grader",
      scope: "repo",
      scopeId: "repo-001",
      rubric: "Grade the docs for clarity.",
    });
    expect(call.operatorType).toBe("GraderAgentOperator");
    expect(call.params).toEqual({
      grader: "docs-quality-grader",
      scope: "repo",
      scope_id: "repo-001",
      rubric: "Grade the docs for clarity.",
    });
  });

  it("includes optional params when provided", () => {
    const call = graderAgent({
      grader: "docs-quality-grader",
      scope: "repo",
      scopeId: "repo-001",
      rubric: "Grade the docs for clarity.",
      model: "gpt-5",
      engine: "codex",
      timeoutSeconds: 90,
    });
    expect(call.params).toEqual({
      grader: "docs-quality-grader",
      scope: "repo",
      scope_id: "repo-001",
      rubric: "Grade the docs for clarity.",
      model: "gpt-5",
      engine: "codex",
      timeout_seconds: 90,
    });
  });
});
