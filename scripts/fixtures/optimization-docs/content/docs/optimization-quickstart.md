# Runnable scheduling example

```yaml
schema_version: 2
id: scheduling-parameters
revision: '1'
strategy: {kind: measurement_driven}
workflows:
  propose: propose.yaml
requirements:
  objective:
    mode: primary
    objective:
      id: makespan
      evaluator: simulator
      measurement: {kind: numeric, unit: simulated_minutes, direction: minimize}
  evaluators:
    simulator: {workflow: evaluate.yaml, revision: deterministic-v1}
  comparison: {kind: exact}
  acceptance_constraints:
    - {id: feasible, evaluator: simulator}
  execution_restrictions: {denied_actions: [], protected_paths: []}
  resource_limits: {elapsed_seconds: 60, max_cycles: 3, max_work: 3, max_evaluations: 6}
  completion:
    - {kind: objective_target, objective: makespan, target: 4}
  stagnation_cycles: 2
```

```yaml
version: '2.0'
mode: workflow_graph
workflow:
  settings:
    entry_task: simulate
    io:
      result_map:
        candidate: '$expr: tasks.simulate.output.patch.candidate'
        evaluation: '$expr: tasks.simulate.output.patch.evaluation'
  tasks:
    - id: simulate
      operator: SetContextOperator
      params:
        patch:
          candidate:
            id: {$expr: triggers.candidate_id}
            artifact_id: {$expr: 'if triggers.stage == "baseline" && triggers.cycle == 1 { "schedule:10" } else { triggers.candidate.artifact_id }'}
            base_artifact_id: schedule:base
            created_under_revision: {$expr: triggers.requirements_revision}
          evaluation:
            id: {$expr: 'triggers.candidate_id + "-sim-" + triggers.stage'}
            run_id: {$expr: triggers.run_id}
            cycle: {$expr: triggers.cycle}
            candidate_id: {$expr: triggers.candidate_id}
            artifact_id: {$expr: 'if triggers.stage == "baseline" && triggers.cycle == 1 { "schedule:10" } else { triggers.candidate.artifact_id }'}
            base_artifact_id: schedule:base
            requirements_revision: {$expr: triggers.requirements_revision}
            evaluator_revisions: {simulator: deterministic-v1}
            measurements:
              makespan:
                status: produced
                measurement: {kind: numeric, unit: simulated_minutes, direction: minimize}
                samples: {$expr: 'if triggers.cycle == 1 && triggers.stage == "baseline" { [10.0] } else if triggers.cycle == 1 { [7.0] } else if triggers.stage == "baseline" { [7.0] } else { [4.0] }'}
            constraints:
              feasible: {evaluator: simulator, status: satisfied, evidence: [deterministic-simulator]}
            completion_checks: {}
      terminal: success
```

```yaml
version: '2.0'
mode: workflow_graph
workflow:
  settings:
    entry_task: search
    io:
      result_map:
        decision: candidate
        proposal_id: '$expr: triggers.candidate_id + "-proposal"'
        candidate: '$expr: #{id: triggers.candidate_id, artifact_id: if triggers.cycle == 1 { "schedule:7" } else { "schedule:4" }, base_artifact_id: "schedule:base", created_under_revision: triggers.requirements_revision}'
        rationale: reduce the deterministic simulated makespan
        selected_observations: []
  tasks:
    - id: search
      operator: NoOpOperator
      terminal: success
```

