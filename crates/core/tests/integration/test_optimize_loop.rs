//! Tier-1 deterministic optimization loop test (spec 069 §11.2).
//!
//! Drives: seed-grader.sh → store entities → deterministic CR synthesis →
//! dimension-matched patch → pytest → git commit → re-grade until convergence.
//!
//! Asserts (oracle from SEED.md):
//!   - Each cycle: open Findings strictly decrease, score strictly increases
//!   - content-coupling: dimension resolved == dimension CR targeted
//!   - Converges (decision: none) within max_cycles
//!   - pytest stays green every cycle
//!   - Zero GhOperator / no `gh` binary invoked

use newton_backend::{
    BackendStore, CreateChangeRequestBody, CreateComponentBody, CreateFindingBody, CreatePlanBody,
    CreateProductBody, CreateRepoBody, SqliteBackendStore,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use uuid::Uuid;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/core  →  go up twice to repo root
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixture_dir() -> PathBuf {
    workspace_root().join("tests/fixtures/repos/widgets-cli")
}

fn grader_script() -> PathBuf {
    workspace_root().join("tests/fixtures/graders/seed-grader.sh")
}

fn patches_dir() -> PathBuf {
    workspace_root().join("tests/fixtures/repos/widgets-cli.patches")
}

fn shim_path_with_failing_gh(shim_dir: &std::path::Path) -> String {
    // Write a fake `gh` script that exits 1 with a clear error.
    let gh_shim = shim_dir.join("gh");
    std::fs::write(
        &gh_shim,
        "#!/bin/sh\necho 'gh invoked — zero-GitHub guard triggered' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let orig = std::env::var("PATH").unwrap_or_default();
    format!("{}:{}", shim_dir.display(), orig)
}

/// Copy the fixture directory to a temp location and init a git repo.
fn setup_repo(tmp: &Path, env_path: &str) {
    let src = fixture_dir();
    copy_dir_all(&src, tmp).expect("copy fixture");

    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(tmp)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.test")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PATH", env_path)
            .status()
            .expect("git");
        assert!(status.success(), "git {:?} failed", args);
    };
    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["commit", "-m", "initial: seeded fixture"]);
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        // Skip __pycache__ and .pyc files — they're stale bytecode from the dev machine
        if name == "__pycache__" {
            continue;
        }
        let ty = entry.file_type()?;
        let dest = dst.join(&name);
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else if entry.path().extension().is_none_or(|e| e != "pyc") {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// Seed product → component → repo and return the repo id.
async fn seed_repo_id(store: &SqliteBackendStore) -> String {
    let now = "2026-01-01T00:00:00Z".to_string();
    let product = store
        .create_product(CreateProductBody {
            name: "widgets-cli-product".to_string(),
        })
        .await
        .unwrap();
    let component = store
        .create_component(CreateComponentBody {
            name: "widgets-cli-component".to_string(),
            product_id: product.id,
            domain: "cli".to_string(),
            owner: "test".to_string(),
            criticality: "low".to_string(),
            autonomy: "autonomous".to_string(),
            trend: 0,
            last_eval: now.clone(),
        })
        .await
        .unwrap();
    let repo = store
        .create_repo(CreateRepoBody {
            name: "widgets-cli".to_string(),
            component_id: component.id,
            owner: "test".to_string(),
            criticality: "low".to_string(),
            autonomy: "autonomous".to_string(),
            exec_status: "idle".to_string(),
            last_eval: now,
        })
        .await
        .unwrap();
    repo.id
}

/// Run the seed grader and return the Assessment as a JSON value.
fn run_seed_grader(repo_id: &str, repo_path: &Path, env_path: &str) -> serde_json::Value {
    let script = grader_script();
    let out = Command::new("bash")
        .arg(script)
        .arg(repo_id)
        .arg(repo_path)
        .env("PATH", env_path)
        .output()
        .expect("seed-grader.sh");
    assert!(
        out.status.success(),
        "seed-grader.sh failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("Assessment JSON")
}

/// Run pytest in the repo dir; returns true if all tests passed.
fn run_pytest(repo_path: &Path, env_path: &str) -> bool {
    Command::new("python3")
        .args(["-m", "pytest", "-q", "--tb=short"])
        .current_dir(repo_path)
        .env("PYTHONPATH", repo_path.join("src"))
        .env("PATH", env_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Apply a patch file with `git apply`. Returns true on success.
fn apply_patch(repo_path: &Path, patch_file: &Path, env_path: &str) -> bool {
    Command::new("git")
        .args(["apply", patch_file.to_str().unwrap()])
        .current_dir(repo_path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PATH", env_path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Commit everything staged in the repo.
fn git_commit(repo_path: &Path, message: &str, env_path: &str) {
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.test")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.test")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("PATH", env_path)
            .status()
            .expect("git");
        assert!(status.success(), "git {:?} failed", args);
    };
    git(&["add", "."]);
    git(&["commit", "-m", message, "--allow-empty"]);
}

// ── Per-cycle record ──────────────────────────────────────────────────────────

#[derive(Debug)]
struct CycleRecord {
    cycle: usize,
    score: f64,
    open_findings: usize,
    resolved_count: usize,
    decision: String,
    targeted_dimension: Option<String>,
    resolved_dimension: Option<String>,
    pytest_passed: bool,
}

// ── Main test ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_optimize_loop_tier1_deterministic() {
    // ── Setup ────────────────────────────────────────────────────────────────
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo_path = tmp.path().to_path_buf();
    let shim_dir = tempfile::tempdir().expect("shim_dir");
    let env_path = shim_path_with_failing_gh(shim_dir.path());
    setup_repo(&repo_path, &env_path);

    let store = Arc::new(SqliteBackendStore::new_in_memory().await.unwrap());

    let repo_id = seed_repo_id(&store).await;

    const MAX_CYCLES: usize = 8;
    let mut trajectory: Vec<CycleRecord> = Vec::new();

    // Track which dimensions have been resolved to detect content-coupling.
    let all_dimensions = [
        "file_decomposition",
        "canonical_placement",
        "abstraction_economy",
        "branching_discipline",
    ];

    for cycle in 1..=MAX_CYCLES {
        // ── Grade ─────────────────────────────────────────────────────────
        let assessment = run_seed_grader(&repo_id, &repo_path, &env_path);
        let score = assessment["overall_score"].as_f64().unwrap_or(0.0);
        let observations: Vec<serde_json::Value> = assessment["observations"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        // ── Reconcile: upsert Findings from observations ────────────────
        let now = chrono::Utc::now().to_rfc3339();
        for obs in &observations {
            let dim = obs["dimension"].as_str().unwrap_or("").to_string();
            let fingerprint = format!("seed-grader:{}:{}", &repo_id, &dim);
            let _ = store
                .create_finding(CreateFindingBody {
                    id: fingerprint.clone(),
                    source: "seed-grader".to_string(),
                    origin: "system".to_string(),
                    component_id: None,
                    module: None,
                    repo_id: Some(repo_id.clone()),
                    kpi_id: None,
                    dimension: dim,
                    location: obs.get("location").cloned(),
                    fingerprint,
                    title: obs["title"].as_str().unwrap_or("").to_string(),
                    why_it_matters: "Reduces maintainability.".to_string(),
                    recommended_action: obs["recommended_action"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    severity: obs["severity"].as_str().unwrap_or("medium").to_string(),
                    risk: "medium".to_string(),
                    confidence: obs["confidence"].as_f64(),
                    evidence: None,
                    expected_value: None,
                    effort: None,
                    status: "awaiting_triage".to_string(),
                    last_seen_at: Some(now.clone()),
                    depends_on: vec![],
                    blocks: vec![],
                })
                .await;
        }

        // Resolve Findings whose dimensions are no longer in the Assessment
        let open_dims: std::collections::HashSet<String> = observations
            .iter()
            .filter_map(|o| o["dimension"].as_str().map(|s| s.to_string()))
            .collect();

        let all_findings = store
            .list_findings(None, Some("repo".to_string()), Some(repo_id.clone()))
            .await
            .unwrap();

        // Resolve findings no longer observed (dimension no longer reported)
        for f in &all_findings {
            if !open_dims.contains(&f.dimension)
                && matches!(
                    f.status.as_str(),
                    "awaiting_triage" | "triaged" | "approved_for_planning"
                )
            {
                let _ = store
                    .patch_finding(
                        &f.id,
                        newton_backend::PatchFindingBody {
                            status: Some("resolved".to_string()),
                            ..Default::default()
                        },
                    )
                    .await;
            }
        }

        let resolved_count = {
            let all = store
                .list_findings(None, Some("repo".to_string()), Some(repo_id.clone()))
                .await
                .unwrap();
            all.iter().filter(|f| f.status == "resolved").count()
        };

        // ── Change Request synthesis ──────────────────────────────────────
        // Re-fetch after resolution so we see the updated statuses.
        let fresh_findings = store
            .list_findings(None, Some("repo".to_string()), Some(repo_id.clone()))
            .await
            .unwrap();

        let open_findings: Vec<_> = fresh_findings
            .iter()
            .filter(|f| {
                matches!(
                    f.status.as_str(),
                    "awaiting_triage" | "triaged" | "approved_for_planning"
                )
            })
            .collect();

        if open_findings.is_empty() {
            // No actionable findings → converged
            trajectory.push(CycleRecord {
                cycle,
                score,
                open_findings: 0,
                resolved_count,
                decision: "none".to_string(),
                targeted_dimension: None,
                resolved_dimension: None,
                pytest_passed: true,
            });
            break;
        }

        let target_finding = open_findings[0];
        let target_dim = target_finding.dimension.clone();

        let cr_id = Uuid::new_v4().to_string();
        store
            .create_change_request(CreateChangeRequestBody {
                id: cr_id.clone(),
                title: format!("Fix {} in widgets-cli", target_dim),
                body: Some(target_finding.recommended_action.clone()),
                origin: "system".to_string(),
                author: None,
                component_id: None,
                repo_id: Some(repo_id.clone()),
                finding_ids: vec![target_finding.id.clone()],
                risk: target_finding.risk.clone(),
                confidence: target_finding.confidence,
            })
            .await
            .unwrap();

        // Create a Plan for this CR
        let plan_id = Uuid::new_v4().to_string();
        store
            .create_plan(CreatePlanBody {
                id: plan_id.clone(),
                title: format!("Plan: fix {}", target_dim),
                linked_change_request_id: cr_id.clone(),
                body: Some(target_finding.recommended_action.clone()),
                status: "ready".to_string(),
                component_id: None,
                repo_id: Some(repo_id.clone()),
                module: None,
                confidence: (target_finding.confidence.unwrap_or(0.8) * 100.0) as i64,
                risk: target_finding.risk.clone(),
                expected_value: None,
                expected_delta: None,
            })
            .await
            .unwrap();

        // ── Deterministic develop: apply dimension-matched patch ───────────
        let patch_file = patches_dir().join(format!("{}.patch", target_dim));
        let patch_applied = if patch_file.exists() {
            apply_patch(&repo_path, &patch_file, &env_path)
        } else {
            false
        };

        // Content-coupling assert: the patch file MUST exist for the targeted dimension.
        assert!(
            patch_applied,
            "cycle {cycle}: no patch found for targeted dimension '{target_dim}' — content-coupling violated"
        );

        // pytest gate: must pass after the patch
        let pytest_ok = run_pytest(&repo_path, &env_path);
        assert!(
            pytest_ok,
            "cycle {cycle}: pytest failed after applying '{target_dim}' patch — behaviour regression"
        );

        git_commit(
            &repo_path,
            &format!("fix({target_dim}): apply deterministic patch"),
            &env_path,
        );

        // Mark Plan complete
        let _ = store
            .patch_plan(
                &plan_id,
                newton_backend::PatchPlanBody {
                    status: Some("complete".to_string()),
                    ..Default::default()
                },
            )
            .await;

        // Content-coupling: re-grade to see which dimension actually resolved.
        // A "fixed the wrong thing" regression would leave target_dim still present.
        let post_patch_assessment = run_seed_grader(&repo_id, &repo_path, &env_path);
        let post_patch_dims: std::collections::HashSet<String> = post_patch_assessment
            ["observations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|o| o["dimension"].as_str().map(|s| s.to_string()))
            .collect();
        // The dimension that was open before but absent after is the one resolved.
        let resolved_dimension: Option<String> = open_findings
            .iter()
            .map(|f| f.dimension.clone())
            .find(|d| !post_patch_dims.contains(d));

        trajectory.push(CycleRecord {
            cycle,
            score,
            open_findings: open_findings.len(),
            resolved_count,
            decision: "propose".to_string(),
            targeted_dimension: Some(target_dim),
            resolved_dimension,
            pytest_passed: pytest_ok,
        });
    }

    // ── Oracle assertions ────────────────────────────────────────────────────

    assert!(!trajectory.is_empty(), "trajectory must not be empty");

    // Last cycle must have converged (decision=none) or we ran out of issues
    let last = trajectory.last().unwrap();
    let final_score = {
        let assessment = run_seed_grader(&repo_id, &repo_path, &env_path);
        assessment["overall_score"].as_f64().unwrap_or(0.0)
    };
    assert!(
        last.decision == "none" || final_score >= 95.0,
        "loop did not converge within {MAX_CYCLES} cycles; final score={final_score}"
    );

    // Scores must be non-decreasing across propose cycles
    let propose_cycles: Vec<&CycleRecord> = trajectory
        .iter()
        .filter(|r| r.decision == "propose")
        .collect();
    for window in propose_cycles.windows(2) {
        let (a, b) = (window[0], window[1]);
        assert!(
            b.score > a.score,
            "score did not strictly increase: cycle {} score={} → cycle {} score={}",
            a.cycle,
            a.score,
            b.cycle,
            b.score
        );
    }

    // open_findings must be non-increasing across cycles
    let propose_with_open: Vec<_> = trajectory
        .iter()
        .filter(|r| r.decision == "propose")
        .collect();
    for window in propose_with_open.windows(2) {
        let (a, b) = (window[0], window[1]);
        assert!(
            b.open_findings < a.open_findings,
            "open findings did not strictly decrease: cycle {} had {} → cycle {} had {}",
            a.cycle,
            a.open_findings,
            b.cycle,
            b.open_findings
        );
    }

    // resolved_count must strictly increase across propose cycles
    let propose_resolved: Vec<&CycleRecord> = trajectory
        .iter()
        .filter(|r| r.decision == "propose")
        .collect();
    for window in propose_resolved.windows(2) {
        let (a, b) = (window[0], window[1]);
        assert!(
            b.resolved_count > a.resolved_count,
            "resolved count did not strictly increase: cycle {} had {} → cycle {} had {}",
            a.cycle,
            a.resolved_count,
            b.cycle,
            b.resolved_count
        );
    }

    // pytest green every cycle
    for rec in &trajectory {
        assert!(
            rec.pytest_passed,
            "pytest failed on cycle {} — behaviour regression",
            rec.cycle
        );
    }

    // Content-coupling: every propose cycle must resolve exactly the targeted dimension.
    // resolved_dimension=None means the patch didn't clear any dimension → regression.
    for rec in &trajectory {
        if rec.decision == "propose" {
            let targeted = rec.targeted_dimension.as_deref().unwrap_or("<none>");
            let resolved = rec
                .resolved_dimension
                .as_deref()
                .unwrap_or("<nothing resolved>");
            assert_eq!(
                targeted, resolved,
                "cycle {}: patch targeted '{}' but post-patch grader shows '{}' resolved",
                rec.cycle, targeted, resolved
            );
        }
    }

    // Final score must be ≥ 95 (all 4 issues fixed)
    assert!(
        final_score >= 95.0,
        "final score {final_score} < 95: not all issues fixed"
    );

    // All 4 dimensions must have been resolved at some point
    let resolved_dims: std::collections::HashSet<&str> = trajectory
        .iter()
        .filter_map(|r| r.resolved_dimension.as_deref())
        .collect();
    for dim in &all_dimensions {
        assert!(
            resolved_dims.contains(*dim),
            "dimension '{}' was never resolved",
            dim
        );
    }

    println!(
        "Tier-1 optimize loop: {} cycles, final score={final_score}, dims resolved={:?}",
        trajectory.len(),
        resolved_dims
    );
}
