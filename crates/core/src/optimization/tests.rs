use super::*;
use newton_types::optimization::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn definition() -> OptimizationDefinition {
    parse_definition(include_str!(
        "../../tests/fixtures/optimization/software-security.yaml"
    ))
    .unwrap()
}

fn capabilities() -> EnforcementCapabilities {
    EnforcementCapabilities {
        denied_actions: [
            ExecutionAction::Merge,
            ExecutionAction::Deploy,
            ExecutionAction::Network,
        ]
        .into(),
        protected_paths: true,
    }
}

fn authority() -> ExecutionAuthority {
    ExecutionAuthority {
        allowed_actions: [
            ExecutionAction::Agent,
            ExecutionAction::Command,
            ExecutionAction::Merge,
        ]
        .into(),
    }
}

fn literal(value: &str) -> ParameterValue {
    ParameterValue::Literal {
        value: json!(value),
    }
}

fn binding() -> DefinitionBinding {
    DefinitionBinding {
        run_id: "run-1".into(),
        context: OptimizationContext {
            id: "repo-1".into(),
            root: "/fixture/repo".into(),
        },
        project_parameters: BTreeMap::new(),
        run_overrides: BTreeMap::new(),
        project_authority: authority(),
        environment_authority: authority(),
        enforcement: capabilities(),
    }
}

fn active() -> RequirementsRevision {
    bind_definition(&definition(), binding())
        .unwrap()
        .requirements
}

fn candidate(id: &str) -> Candidate {
    Candidate {
        id: id.into(),
        artifact_id: format!("sha256:{id}"),
        base_artifact_id: "sha256:base".into(),
        created_under_revision: 1,
    }
}

fn evaluation(
    active: &RequirementsRevision,
    candidate: &Candidate,
    samples: Vec<f64>,
) -> CandidateEvaluation {
    let spec = super::validation::objective_specs(&active.requirements)[0];
    CandidateEvaluation {
        id: format!("eval-{}", candidate.id),
        run_id: "run-1".into(),
        cycle: 1,
        candidate_id: candidate.id.clone(),
        artifact_id: candidate.artifact_id.clone(),
        base_artifact_id: candidate.base_artifact_id.clone(),
        requirements_revision: active.revision,
        evaluator_revisions: active
            .requirements
            .evaluators
            .iter()
            .map(|(id, evaluator)| (id.clone(), evaluator.revision.clone()))
            .collect(),
        measurements: [(
            spec.id.clone(),
            ObjectiveMeasurement::Produced {
                measurement: spec.measurement.clone(),
                samples,
            },
        )]
        .into(),
        constraints: [(
            "tests_preserved".into(),
            CheckEvidence {
                evaluator: "tests".into(),
                status: CheckStatus::Satisfied,
                evidence: vec!["artifact:test-report".into()],
                judged_by: None,
            },
        )]
        .into(),
        completion_checks: BTreeMap::new(),
    }
}

fn accepted(active: &RequirementsRevision, samples: Vec<f64>) -> AcceptedResult {
    let candidate = candidate("baseline");
    let evaluation = evaluation(active, &candidate, samples);
    evaluate_candidate("run-1", active, &candidate, &evaluation, None)
        .unwrap()
        .accepted_result
        .unwrap()
}

fn update_authority() -> RequirementsUpdateAuthority {
    RequirementsUpdateAuthority {
        actor: "local-owner".into(),
        may_update_requirements: true,
        may_update_evaluators: true,
        may_update_execution_restrictions: true,
    }
}

fn update(active: &RequirementsRevision) -> RequirementsUpdate {
    RequirementsUpdate {
        base_revision: active.revision,
        requirements: active.requirements.clone(),
        parameter_overrides: BTreeMap::new(),
    }
}

fn outcome_context(reason: OptimizationStopReason) -> OutcomeContext {
    OutcomeContext {
        stop_reason: reason,
        usage: ResourceUsage::default(),
        blocked_work: vec!["blocked-finding".into()],
        historical_result_ids: Vec::new(),
        diagnostics: vec!["original-state:sha256:base".into()],
    }
}

#[test]
fn declarative_parser_is_strict_and_versioned() {
    let original = definition();
    let yaml = serde_yaml::to_string(&original).unwrap();
    assert_eq!(parse_definition(&yaml).unwrap(), original);
    assert!(parse_definition(&format!("{yaml}\nexecute: malicious-command\n")).is_err());
    let mut invalid = original.clone();
    invalid.schema_version = 2;
    assert!(validate_definition(&invalid).is_err());
    invalid = original.clone();
    invalid.requirements.completion.clear();
    assert!(validate_definition(&invalid).is_err());
    invalid = original.clone();
    invalid.requirements.evaluators.remove("tests");
    assert!(validate_definition(&invalid).is_err());
    invalid = original;
    invalid.requirements.resource_limits.max_work = 0;
    assert!(validate_definition(&invalid).is_err());
}

#[test]
fn binding_precedence_snapshots_and_secret_safe_inspection() {
    let mut source = definition();
    source.defaults.insert(
        "credential".into(),
        ParameterValue::SecretReference {
            reference: "secret-provider:private-name".into(),
        },
    );
    let mut input = binding();
    input
        .project_parameters
        .insert("agent".into(), literal("cursor"));
    input
        .project_parameters
        .insert("unrelated".into(), literal("preserved"));
    input.run_overrides.insert("agent".into(), literal("codex"));
    let bound = bind_definition(&source, input).unwrap();
    source.revision = "changed-on-disk".into();
    assert_eq!(bound.definition.revision, "fixture-v1");
    assert_eq!(bound.parameters["agent"], literal("codex"));
    assert_eq!(bound.parameters["unrelated"], literal("preserved"));
    let preview = inspect_binding(&bound);
    assert_eq!(preview["parameters"]["credential"], "[secret reference]");
    assert!(!preview.to_string().contains("private-name"));
    let restored: BoundOptimizationDefinition =
        serde_json::from_value(serde_json::to_value(&bound).unwrap()).unwrap();
    assert_eq!(restored, bound);
    let mut second = binding();
    second.context.id = "repo-2".into();
    assert_eq!(
        bind_definition(&bound.definition, second)
            .unwrap()
            .definition,
        bound.definition
    );
}

#[test]
fn settings_never_grant_authority_and_unsupported_restrictions_fail_closed() {
    let mut input = binding();
    input.run_overrides.insert(
        "merge".into(),
        ParameterValue::Literal { value: json!(true) },
    );
    input
        .environment_authority
        .allowed_actions
        .remove(&ExecutionAction::Command);
    let bound = bind_definition(&definition(), input).unwrap();
    assert!(authorize_action(&bound.authority, ExecutionAction::Merge).is_err());
    assert!(authorize_action(&bound.authority, ExecutionAction::Command).is_err());
    authorize_action(&bound.authority, ExecutionAction::Agent).unwrap();
    let mut unsupported = binding();
    unsupported.enforcement = EnforcementCapabilities::default();
    assert!(matches!(
        bind_definition(&definition(), unsupported),
        Err(OptimizationError::UnsupportedRestriction(_))
    ));
    let mut unsupported_paths = binding();
    unsupported_paths.enforcement.protected_paths = false;
    assert!(bind_definition(&definition(), unsupported_paths).is_err());
}

#[test]
fn initial_qualification_does_not_claim_improvement_or_require_completion() {
    let active = active();
    let result = accepted(&active, vec![500.0]);
    assert_eq!(
        result.evaluation.measurements["critical_vulnerabilities"],
        evaluation(&active, &result.candidate, vec![500.0]).measurements
            ["critical_vulnerabilities"]
    );
    assert_eq!(
        assess_completion("run-1", &active, Some(&result)).status,
        CheckStatus::Violated
    );
    let evaluation = evaluation(&active, &result.candidate, vec![500.0]);
    let decision =
        evaluate_candidate("run-1", &active, &result.candidate, &evaluation, None).unwrap();
    assert_eq!(
        decision.disposition,
        CandidateDisposition::InitialQualification
    );
    assert_eq!(
        decision.comparisons["critical_vulnerabilities"],
        ComparisonOutcome::Initial
    );
}

#[test]
fn initially_unmet_unknown_and_human_constraints_never_qualify() {
    let mut active = active();
    let candidate = candidate("repair");
    for (status, expected) in [
        (CheckStatus::Violated, CandidateDisposition::Rejected),
        (CheckStatus::Unknown, CandidateDisposition::Inconclusive),
    ] {
        let mut evidence = evaluation(&active, &candidate, vec![0.0]);
        evidence
            .constraints
            .get_mut("tests_preserved")
            .unwrap()
            .status = status;
        let decision = evaluate_candidate("run-1", &active, &candidate, &evidence, None).unwrap();
        assert_eq!(decision.disposition, expected);
        assert!(decision.accepted_result.is_none());
    }
    active.requirements.acceptance_constraints[0].human_judged = true;
    let mut evidence = evaluation(&active, &candidate, vec![0.0]);
    assert!(
        evaluate_candidate("run-1", &active, &candidate, &evidence, None)
            .unwrap()
            .accepted_result
            .is_none()
    );
    evidence
        .constraints
        .get_mut("tests_preserved")
        .unwrap()
        .judged_by = Some("reviewer".into());
    assert!(
        evaluate_candidate("run-1", &active, &candidate, &evidence, None)
            .unwrap()
            .accepted_result
            .is_some()
    );
}

#[test]
fn exact_comparison_preserves_ties_rejects_regressions_and_honors_direction() {
    for direction in [Direction::Minimize, Direction::Maximize] {
        let mut active = active();
        if let ObjectiveMode::Primary { objective } = &mut active.requirements.objective {
            objective.measurement = MeasurementKind::Numeric {
                unit: "count".into(),
                direction,
            };
        }
        let baseline = accepted(&active, vec![10.0]);
        let candidate = candidate("candidate");
        let improvement = if direction == Direction::Minimize {
            9.0
        } else {
            11.0
        };
        let regression = if direction == Direction::Minimize {
            11.0
        } else {
            9.0
        };
        for (score, expected) in [
            (10.0, CandidateDisposition::Unchanged),
            (improvement, CandidateDisposition::Improvement),
            (regression, CandidateDisposition::Rejected),
        ] {
            let evidence = evaluation(&active, &candidate, vec![score]);
            let decision =
                evaluate_candidate("run-1", &active, &candidate, &evidence, Some(&baseline))
                    .unwrap();
            assert_eq!(decision.disposition, expected);
            assert_eq!(
                decision.accepted_result.is_some(),
                expected == CandidateDisposition::Improvement
            );
        }
    }
}

#[test]
fn noisy_comparison_requires_complete_repeats_and_separated_ranges() {
    let mut active = active();
    active.requirements.comparison = ComparisonPolicy::Repeated {
        samples: 3,
        min_improvement: 2.0,
    };
    let baseline = accepted(&active, vec![10.0, 11.0, 12.0]);
    let candidate = candidate("noisy");
    for (samples, expected) in [
        (vec![7.0, 8.0, 7.0], CandidateDisposition::Improvement),
        (vec![9.0, 10.0, 11.0], CandidateDisposition::Inconclusive),
    ] {
        let decision = evaluate_candidate(
            "run-1",
            &active,
            &candidate,
            &evaluation(&active, &candidate, samples),
            Some(&baseline),
        )
        .unwrap();
        assert_eq!(decision.disposition, expected);
        assert_eq!(
            decision.accepted_result.is_some(),
            expected == CandidateDisposition::Improvement
        );
    }
    assert!(evaluate_candidate(
        "run-1",
        &active,
        &candidate,
        &evaluation(&active, &candidate, vec![1.0]),
        Some(&baseline)
    )
    .is_err());
    active.requirements.resource_limits.max_evaluations = 2;
    assert!(validate_requirements(&active.requirements).is_err());
}

#[test]
fn stale_or_changed_state_and_evaluator_evidence_is_rejected() {
    let active = active();
    let candidate = candidate("candidate");
    for field in [
        "run",
        "candidate",
        "artifact",
        "base",
        "requirements",
        "evaluator",
    ] {
        let mut evidence = evaluation(&active, &candidate, vec![0.0]);
        match field {
            "run" => evidence.run_id = "different".into(),
            "candidate" => evidence.candidate_id = "different".into(),
            "artifact" => evidence.artifact_id = "different".into(),
            "base" => evidence.base_artifact_id = "different".into(),
            "requirements" => evidence.requirements_revision = 0,
            "evaluator" => {
                evidence
                    .evaluator_revisions
                    .insert("security".into(), "weakened".into());
            }
            _ => unreachable!(),
        }
        assert!(
            evaluate_candidate("run-1", &active, &candidate, &evidence, None).is_err(),
            "{field}"
        );
    }
    let result = accepted(&active, vec![0.0]);
    validate_promotion(
        "run-1",
        &active,
        &result,
        &result.candidate.artifact_id,
        &result.candidate.base_artifact_id,
    )
    .unwrap();
    assert!(validate_promotion(
        "run-1",
        &active,
        &result,
        "modified-after-evaluation",
        &result.candidate.base_artifact_id
    )
    .is_err());
    assert!(validate_promotion(
        "run-1",
        &active,
        &result,
        &result.candidate.artifact_id,
        "changed-integration-base"
    )
    .is_err());
}

#[test]
fn grade_bounds_and_operational_errors_are_not_native_measurements() {
    let mut active = active();
    if let ObjectiveMode::Primary { objective } = &mut active.requirements.objective {
        objective.measurement = MeasurementKind::Grade {
            dimension: "security".into(),
        };
    }
    let candidate = candidate("candidate");
    for invalid in [101.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(evaluate_candidate(
            "run-1",
            &active,
            &candidate,
            &evaluation(&active, &candidate, vec![invalid]),
            None
        )
        .is_err());
    }
    assert!(evaluate_candidate(
        "run-1",
        &active,
        &candidate,
        &evaluation(&active, &candidate, vec![90.0]),
        None
    )
    .unwrap()
    .accepted_result
    .is_some());
    let mut failed = evaluation(&active, &candidate, vec![90.0]);
    failed.measurements.insert(
        "critical_vulnerabilities".into(),
        ObjectiveMeasurement::Error {
            message: "grader unavailable".into(),
        },
    );
    assert!(
        evaluate_candidate("run-1", &active, &candidate, &failed, None)
            .unwrap_err()
            .to_string()
            .contains("grader unavailable")
    );
    let wire = serde_json::to_value(&failed).unwrap();
    assert_eq!(
        wire["measurements"]["critical_vulnerabilities"]["status"],
        "error"
    );
}

#[test]
fn revisions_are_pending_until_acknowledged_and_reject_stale_updates() {
    let active = active();
    let mut request = update(&active);
    request
        .parameter_overrides
        .insert("agent".into(), literal("cursor"));
    let pending = propose_revision(
        &active,
        &authority(),
        request,
        &update_authority(),
        &capabilities(),
    )
    .unwrap();
    assert_eq!(pending.status, RequirementsRevisionStatus::Pending);
    assert_eq!(active.revision, 1);
    let changed = acknowledge_revision(
        &active,
        &pending,
        RevisionBoundary::default(),
        &capabilities(),
    )
    .unwrap();
    assert_eq!(changed.active.revision, 2);
    assert_eq!(changed.active.parameters["agent"], literal("cursor"));
    assert_eq!(
        changed.superseded.status,
        RequirementsRevisionStatus::Superseded
    );
    assert!(matches!(
        acknowledge_revision(
            &changed.active,
            &pending,
            RevisionBoundary::default(),
            &capabilities()
        ),
        Err(OptimizationError::StaleRevision { .. })
    ));
    let rejected = reject_revision(&pending, "owner declined").unwrap();
    assert_eq!(rejected.status, RequirementsRevisionStatus::Rejected);
    assert!(acknowledge_revision(
        &active,
        &rejected,
        RevisionBoundary::default(),
        &capabilities()
    )
    .is_err());
}

#[test]
fn execution_restrictions_require_pause_and_disclose_prior_actions() {
    let active = active();
    let mut request = update(&active);
    request
        .requirements
        .execution_restrictions
        .denied_actions
        .insert(ExecutionAction::Network);
    let pending = propose_revision(
        &active,
        &authority(),
        request,
        &update_authority(),
        &capabilities(),
    )
    .unwrap();
    assert!(acknowledge_revision(
        &active,
        &pending,
        RevisionBoundary::default(),
        &capabilities()
    )
    .is_err());
    assert_eq!(pending.status, RequirementsRevisionStatus::Pending);
    let boundary = RevisionBoundary {
        affected_work_paused: true,
        prior_actions: vec!["network request already completed".into()],
    };
    let changed = acknowledge_revision(&active, &pending, boundary, &capabilities()).unwrap();
    assert_eq!(changed.active.prior_actions.len(), 1);
    assert!(changed
        .active
        .requirements
        .execution_restrictions
        .denied_actions
        .contains(&ExecutionAction::Network));
}

#[test]
fn evaluator_changes_require_explicit_authority_and_stale_baselines_need_rechecking() {
    let active = active();
    let baseline = accepted(&active, vec![8.0]);
    let mut request = update(&active);
    request
        .requirements
        .evaluators
        .get_mut("security")
        .unwrap()
        .revision = "new-rubric".into();
    let mut denied = update_authority();
    denied.may_update_evaluators = false;
    assert!(matches!(
        propose_revision(
            &active,
            &authority(),
            request.clone(),
            &denied,
            &capabilities()
        ),
        Err(OptimizationError::Unauthorized(_))
    ));
    let pending = propose_revision(
        &active,
        &authority(),
        request,
        &update_authority(),
        &capabilities(),
    )
    .unwrap();
    let next = acknowledge_revision(
        &active,
        &pending,
        RevisionBoundary::default(),
        &capabilities(),
    )
    .unwrap()
    .active;
    let candidate = candidate("in-flight");
    let current_evidence = evaluation(&next, &candidate, vec![0.0]);
    let decision = evaluate_candidate(
        "run-1",
        &next,
        &candidate,
        &current_evidence,
        Some(&baseline),
    )
    .unwrap();
    assert_eq!(decision.disposition, CandidateDisposition::Inconclusive);
    assert!(decision.accepted_result.is_none());
    assert!(evaluate_candidate(
        "run-1",
        &next,
        &candidate,
        &evaluation(&active, &candidate, vec![0.0]),
        None
    )
    .is_err());
}

#[test]
fn outcome_distinguishes_progress_completion_stops_and_historical_results() {
    let active = active();
    let best = accepted(&active, vec![3.0]);
    let outcome = build_outcome(
        "run-1",
        &active,
        Some(&best),
        outcome_context(OptimizationStopReason::ResourceLimit),
    )
    .unwrap();
    assert!(!outcome.no_acceptable_result_found);
    assert_eq!(outcome.completion.status, CheckStatus::Violated);
    assert_eq!(outcome.blocked_work, vec!["blocked-finding"]);
    assert!(build_outcome(
        "run-1",
        &active,
        Some(&best),
        outcome_context(OptimizationStopReason::Completed)
    )
    .is_err());
    for reason in [
        OptimizationStopReason::NoActionableWork,
        OptimizationStopReason::OperationalFailure,
        OptimizationStopReason::Cancelled,
    ] {
        let outcome = build_outcome("run-1", &active, None, outcome_context(reason)).unwrap();
        assert_eq!(outcome.stop_reason, reason);
        assert!(outcome.no_acceptable_result_found);
        assert_eq!(outcome.completion.status, CheckStatus::Unknown);
    }
    let complete = accepted(&active, vec![0.0]);
    assert_eq!(
        build_outcome(
            "run-1",
            &active,
            Some(&complete),
            outcome_context(OptimizationStopReason::Completed)
        )
        .unwrap()
        .completion
        .status,
        CheckStatus::Satisfied
    );
    let mut next = active.clone();
    next.revision = 2;
    let outcome = build_outcome(
        "run-1",
        &next,
        Some(&complete),
        outcome_context(OptimizationStopReason::ResourceLimit),
    )
    .unwrap();
    assert!(outcome.accepted_result.is_none());
    assert!(outcome
        .historical_result_ids
        .contains(&complete.candidate.id));
}

#[test]
fn completion_checks_are_independent_and_noisy_targets_need_all_samples() {
    let mut active = active();
    active
        .requirements
        .completion
        .push(CompletionCriterion::Check {
            constraint: AcceptanceConstraint {
                id: "review".into(),
                evaluator: "tests".into(),
                human_judged: true,
            },
        });
    let mut best = accepted(&active, vec![0.0]);
    assert_eq!(
        assess_completion("run-1", &active, Some(&best)).status,
        CheckStatus::Unknown
    );
    best.evaluation.completion_checks.insert(
        "review".into(),
        CheckEvidence {
            evaluator: "tests".into(),
            status: CheckStatus::Satisfied,
            evidence: vec!["review-record".into()],
            judged_by: Some("owner".into()),
        },
    );
    assert_eq!(
        assess_completion("run-1", &active, Some(&best)).status,
        CheckStatus::Satisfied
    );
    active.requirements.completion.pop();
    active.requirements.comparison = ComparisonPolicy::Repeated {
        samples: 2,
        min_improvement: 1.0,
    };
    let best = accepted(&active, vec![0.0, 1.0]);
    assert_eq!(
        assess_completion("run-1", &active, Some(&best)).status,
        CheckStatus::Violated
    );
}

#[test]
fn every_finite_resource_counter_is_exposed() {
    let limits = definition().requirements.resource_limits;
    assert!(exhausted_resources(&limits, &ResourceUsage::default()).is_empty());
    let usage = ResourceUsage {
        elapsed_seconds: limits.elapsed_seconds,
        cycles: limits.max_cycles,
        work: limits.max_work,
        evaluations: limits.max_evaluations,
    };
    assert_eq!(
        exhausted_resources(&limits, &usage),
        vec!["elapsed_seconds", "cycles", "work", "evaluations"]
    );
}

#[test]
fn threshold_targets_are_independent_and_no_cosmetic_scalar_controls_them() {
    let mut active = active();
    let security = ObjectiveSpec {
        id: "security".into(),
        evaluator: "security".into(),
        measurement: MeasurementKind::Grade {
            dimension: "security".into(),
        },
    };
    let tests = ObjectiveSpec {
        id: "tests".into(),
        evaluator: "tests".into(),
        measurement: MeasurementKind::Grade {
            dimension: "tests".into(),
        },
    };
    active.requirements.objective = ObjectiveMode::Thresholds {
        objectives: vec![
            ThresholdObjective {
                objective: security.clone(),
                target: 90.0,
                regression_delta: 5.0,
                no_progress_cycles: 3,
            },
            ThresholdObjective {
                objective: tests.clone(),
                target: 50.0,
                regression_delta: 2.0,
                no_progress_cycles: 2,
            },
        ],
    };
    active.requirements.completion = vec![CompletionCriterion::ObjectiveTarget {
        objective: "security".into(),
        target: 90.0,
    }];
    let candidate = candidate("multi");
    let mut evidence = evaluation(&active, &candidate, vec![100.0]);
    evidence.measurements.insert(
        "tests".into(),
        ObjectiveMeasurement::Produced {
            measurement: tests.measurement.clone(),
            samples: vec![49.0],
        },
    );
    let accepted = evaluate_candidate("run-1", &active, &candidate, &evidence, None)
        .unwrap()
        .accepted_result
        .unwrap();
    assert_eq!(
        assess_completion("run-1", &active, Some(&accepted)).status,
        CheckStatus::Violated
    );
    evidence.measurements.insert(
        "tests".into(),
        ObjectiveMeasurement::Produced {
            measurement: tests.measurement,
            samples: vec![50.0],
        },
    );
    let accepted = evaluate_candidate("run-1", &active, &candidate, &evidence, None)
        .unwrap()
        .accepted_result
        .unwrap();
    assert_eq!(
        assess_completion("run-1", &active, Some(&accepted)).status,
        CheckStatus::Satisfied
    );
    assert_eq!(
        accepted
            .evaluation
            .measurements
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        ["security".into(), "tests".into()].into()
    );
}

#[test]
fn generated_schemas_match_definition_evidence_and_update_serialization() {
    let definition = definition();
    let active = active();
    let candidate = candidate("schema");
    let evidence = evaluation(&active, &candidate, vec![3.0]);
    for (schema, value) in [
        (
            optimization_definition_schema(),
            serde_json::to_value(&definition).unwrap(),
        ),
        (
            candidate_evaluation_schema(),
            serde_json::to_value(&evidence).unwrap(),
        ),
        (
            requirements_update_schema(),
            serde_json::to_value(update(&active)).unwrap(),
        ),
    ] {
        let validator = jsonschema::JSONSchema::compile(&schema).unwrap();
        assert!(
            validator.is_valid(&value),
            "serialized DTO must match its generated schema"
        );
        let mut unknown = value.clone();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unrecognized_field".into(), json!(true));
        assert!(!validator.is_valid(&unknown));
    }
    let schema = candidate_evaluation_schema();
    let validator = jsonschema::JSONSchema::compile(&schema).unwrap();
    let mut invalid = serde_json::to_value(&evidence).unwrap();
    invalid["measurements"]["critical_vulnerabilities"]["status"] = json!("passed");
    assert!(!validator.is_valid(&invalid));
    assert!(serde_json::from_value::<CandidateEvaluation>(invalid).is_err());
}

#[test]
fn one_cycle_stop_does_not_claim_budget_exhaustion_or_completion() {
    let active = active();
    let best = accepted(&active, vec![3.0]);
    let outcome = build_outcome(
        "run-1",
        &active,
        Some(&best),
        outcome_context(OptimizationStopReason::CycleComplete),
    )
    .unwrap();
    assert_eq!(outcome.stop_reason, OptimizationStopReason::CycleComplete);
    assert_eq!(outcome.completion.status, CheckStatus::Violated);
    assert_eq!(
        serde_json::to_value(&outcome).unwrap()["stop_reason"],
        "cycle_complete"
    );
}
