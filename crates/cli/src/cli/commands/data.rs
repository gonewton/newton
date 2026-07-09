use crate::cli::args::{DataArgs, DataVerb};
use crate::cli::exit::CliExit;
use crate::cli::WorkspacePaths;
use newton_backend::BackendStore;
use std::fs;

pub async fn data(args: DataArgs) -> anyhow::Result<()> {
    if args.file.is_some() && args.body.is_some() {
        return Err(CliExit::new(
            1,
            "DATA-001: --file and --body are mutually exclusive; provide at most one",
        )
        .into());
    }

    let workspace = match args.workspace {
        Some(ref p) => p.clone(),
        None => std::env::current_dir()?,
    };
    let state_dir =
        crate::cli::workspace_paths::resolve_state_dir(&workspace, args.state_dir.as_deref());
    let workspace_paths = WorkspacePaths::with_state_dir(workspace, state_dir);
    let db_url = workspace_paths.backend_sqlite_url();
    let store = match newton_backend::SqliteBackendStore::new(&db_url).await {
        Ok(s) => s,
        Err(e) => {
            return Err(
                CliExit::new(1, format!("Failed to open backend store: {}", e.message)).into(),
            );
        }
    };

    let body_value: Option<serde_json::Value> = if let Some(ref path) = args.file {
        let raw = if path.to_string_lossy() == "-" {
            use std::io::Read;
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            s
        } else {
            fs::read_to_string(path)?
        };
        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => Some(v),
            Err(e) => {
                return Err(CliExit::new(1, format!("DATA-004: invalid JSON in body: {e}")).into());
            }
        }
    } else if let Some(ref s) = args.body {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => Some(v),
            Err(e) => {
                return Err(
                    CliExit::new(1, format!("DATA-004: invalid JSON in --body: {e}")).into(),
                );
            }
        }
    } else {
        None
    };

    let resource = args.resource.as_str();

    if (args.run_id.is_some() || args.kpi_id.is_some())
        && resource != "grades"
        && resource != "optimize-cycles"
    {
        return Err(CliExit::new(
            1,
            "DATA-006: --run-id/--kpi-id are only supported with: resource=grades, optimize-cycles",
        )
        .into());
    }
    if (args.scope.is_some()
        || args.scope_id.is_some()
        || args.source.is_some()
        || args.limit.is_some())
        && resource != "eval-runs"
    {
        return Err(CliExit::new(
            1,
            "DATA-008: --scope/--scope-id/--source/--limit are only supported with: resource=eval-runs",
        )
        .into());
    }

    let valid_resources = [
        "product",
        "products",
        "component",
        "components",
        "repo",
        "repos",
        "module",
        "modules",
        "module-dependency",
        "module-dependencies",
        "kpi",
        "kpis",
        "eval-run",
        "eval-runs",
        "grade",
        "grades",
        "finding",
        "findings",
        "change-request",
        "change-requests",
        "plan",
        "plans",
        "optimize-run",
        "optimize-runs",
        "optimize-cycle",
        "optimize-cycles",
    ];
    if !valid_resources.contains(&resource) {
        return Err(CliExit::new(1, format!("DATA-003: unknown resource '{resource}'; must be one of: product, products, component, components, repo, repos, module, modules, module-dependency, module-dependencies, kpi, kpis, eval-run, eval-runs, grade, grades, finding, findings, change-request, change-requests, plan, plans, optimize-run, optimize-runs, optimize-cycle, optimize-cycles")).into());
    }

    if matches!(args.verb, DataVerb::Post | DataVerb::Put | DataVerb::Patch) && body_value.is_none()
    {
        return Err(CliExit::new(
            1,
            format!("DATA-005: --file or --body is required for {}", args.verb),
        )
        .into());
    }

    let needs_id = match args.verb {
        DataVerb::Get => !matches!(
            resource,
            "products"
                | "components"
                | "repos"
                | "modules"
                | "module-dependencies"
                | "kpis"
                | "eval-runs"
                | "grades"
                | "findings"
                | "change-requests"
                | "plans"
                | "optimize-runs"
                | "optimize-cycles"
        ),
        DataVerb::Post => false,
        DataVerb::Put | DataVerb::Patch | DataVerb::Delete => true,
    };
    if needs_id && args.id.is_none() {
        return Err(CliExit::new(
            1,
            format!("DATA-002: ID is required for {} {}", args.verb, resource),
        )
        .into());
    }

    if args.dry_run {
        if matches!(args.verb, DataVerb::Post | DataVerb::Put | DataVerb::Patch) {
            if let Some(ref v) = body_value {
                match resource {
                    "component" | "components" => {
                        if let Some(product_id) = v.get("productId").and_then(|p| p.as_str()) {
                            if let Err(e) = store.get_product(product_id).await {
                                return Err(CliExit::new(
                                    1,
                                    format!(
                                        "[dry-run] FK validation failed: productId '{}' not found: {}",
                                        product_id, e.message
                                    ),
                                )
                                .into());
                            }
                        }
                    }
                    "repo" | "repos" => {
                        if let Some(component_id) = v.get("componentId").and_then(|c| c.as_str()) {
                            if let Err(e) = store.get_component(component_id).await {
                                return Err(CliExit::new(1, format!("[dry-run] FK validation failed: componentId '{}' not found: {}", component_id, e.message)).into());
                            }
                        }
                    }
                    "module" | "modules" => {
                        if let Some(repo_id) = v.get("repoId").and_then(|r| r.as_str()) {
                            if let Err(e) = store.get_repo(repo_id).await {
                                return Err(CliExit::new(
                                    1,
                                    format!(
                                        "[dry-run] FK validation failed: repoId '{}' not found: {}",
                                        repo_id, e.message
                                    ),
                                )
                                .into());
                            }
                        }
                    }
                    "eval-run" | "eval-runs" => {
                        let scope = v.get("scope").and_then(|s| s.as_str()).unwrap_or("");
                        let scope_id = v.get("scopeId").and_then(|s| s.as_str()).unwrap_or("");
                        if scope.is_empty() || scope_id.is_empty() {
                            return Err(CliExit::new(
                                1,
                                "[dry-run] FK validation failed: scope and scopeId are required",
                            )
                            .into());
                        }
                        let fk_result = match scope {
                            "product" => store.get_product(scope_id).await.map(|_| ()),
                            "component" => store.get_component(scope_id).await.map(|_| ()),
                            "repo" => store.get_repo(scope_id).await.map(|_| ()),
                            "module" => store.get_module(scope_id).await.map(|_| ()),
                            _ => Err(newton_backend::err_validation(
                                "scope must be one of: product, component, repo, module",
                            )),
                        };
                        if let Err(e) = fk_result {
                            return Err(CliExit::new(
                                1,
                                format!(
                                    "[dry-run] FK validation failed: {} '{}' not found: {}",
                                    scope, scope_id, e.message
                                ),
                            )
                            .into());
                        }
                    }
                    "grade" | "grades" => {
                        let run_id = v.get("runId").and_then(|r| r.as_str());
                        let Some(run_id) = run_id else {
                            return Err(CliExit::new(
                                1,
                                "[dry-run] FK validation failed: runId is required",
                            )
                            .into());
                        };
                        if let Err(e) = store.get_eval_run(run_id).await {
                            return Err(CliExit::new(
                                1,
                                format!(
                                    "[dry-run] FK validation failed: runId '{}' not found: {}",
                                    run_id, e.message
                                ),
                            )
                            .into());
                        }
                        if let Some(kpi_id) = v.get("kpiId").and_then(|k| k.as_str()) {
                            if let Err(e) = store.get_kpi(kpi_id).await {
                                return Err(CliExit::new(
                                    1,
                                    format!(
                                        "[dry-run] FK validation failed: kpiId '{}' not found: {}",
                                        kpi_id, e.message
                                    ),
                                )
                                .into());
                            }
                        }
                    }
                    _ => {}
                }
                eprintln!("[dry-run] validated payload (no DB write):");
                println!("{}", serde_json::to_string_pretty(v)?);
            } else {
                eprintln!("[dry-run] no body to validate");
            }
        } else {
            eprintln!(
                "[dry-run] no-op for {} (only POST/PUT/PATCH validate body)",
                args.verb
            );
        }
        return Ok(());
    }

    match dispatch_data(&store, &args, body_value).await {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        Err(msg) => Err(CliExit::new(1, msg).into()),
    }
}

async fn dispatch_data(
    store: &newton_backend::SqliteBackendStore,
    args: &DataArgs,
    body: Option<serde_json::Value>,
) -> std::result::Result<serde_json::Value, String> {
    fn api_err(e: newton_types::ApiError) -> String {
        format!("{}: {}", e.code, e.message)
    }

    fn to_json<T: serde::Serialize>(v: T) -> std::result::Result<serde_json::Value, String> {
        serde_json::to_value(v).map_err(|e| format!("serialize error: {e}"))
    }

    fn parse_body<T: serde::de::DeserializeOwned>(
        body: Option<serde_json::Value>,
    ) -> std::result::Result<T, String> {
        match body {
            None => Err("body required".to_string()),
            Some(v) => {
                serde_json::from_value(v).map_err(|e| format!("DATA-004: body parse error: {e}"))
            }
        }
    }

    let verb = &args.verb;
    let resource = args.resource.as_str();
    let id = args.id.as_deref().unwrap_or("");
    let grade_run_id = args.run_id.as_deref();
    let grade_kpi_id = args.kpi_id.as_deref();
    let eval_scope = args.scope.as_deref();
    let eval_scope_id = args.scope_id.as_deref();
    let eval_source = args.source.as_deref();
    let eval_limit = args.limit;

    match (verb, resource) {
        (DataVerb::Get, "products") => store
            .list_products()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "product") => store
            .get_product(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "product" | "products") => {
            let b = parse_body::<newton_backend::CreateProductBody>(body)?;
            store
                .create_product(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "product" | "products") => {
            let b = parse_body::<newton_backend::PutProductBody>(body)?;
            store
                .put_product(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "product" | "products") => {
            let b = parse_body::<newton_backend::PatchProductBody>(body)?;
            store
                .patch_product(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "product" | "products") => store
            .delete_product(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        (DataVerb::Get, "components") => store
            .list_components()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "component") => store
            .get_component(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "component" | "components") => {
            let b = parse_body::<newton_backend::CreateComponentBody>(body)?;
            store
                .create_component(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "component" | "components") => {
            let b = parse_body::<newton_backend::PutComponentBody>(body)?;
            store
                .put_component(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "component" | "components") => {
            let b = parse_body::<newton_backend::PatchComponentBody>(body)?;
            store
                .patch_component(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "component" | "components") => store
            .delete_component(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        (DataVerb::Get, "repos") => store.list_repos().await.map_err(api_err).and_then(to_json),
        (DataVerb::Get, "repo") => store.get_repo(id).await.map_err(api_err).and_then(to_json),
        (DataVerb::Post, "repo" | "repos") => {
            let b = parse_body::<newton_backend::CreateRepoBody>(body)?;
            store
                .create_repo(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "repo" | "repos") => {
            let b = parse_body::<newton_backend::PutRepoBody>(body)?;
            store
                .put_repo(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "repo" | "repos") => {
            let b = parse_body::<newton_backend::PatchRepoBody>(body)?;
            store
                .patch_repo(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "repo" | "repos") => store
            .delete_repo(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        (DataVerb::Get, "modules") => store
            .list_modules()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "module") => store
            .get_module(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "module" | "modules") => {
            let b = parse_body::<newton_backend::CreateModuleBody>(body)?;
            store
                .create_module(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Put, "module" | "modules") => {
            let b = parse_body::<newton_backend::PutModuleBody>(body)?;
            store
                .put_module(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "module" | "modules") => {
            let b = parse_body::<newton_backend::PatchModuleBody>(body)?;
            store
                .patch_module(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "module" | "modules") => store
            .delete_module(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        (DataVerb::Get, "module-dependencies") => store
            .list_module_dependencies()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "module-dependency") => store
            .get_module_dependency(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Patch, "module-dependency" | "module-dependencies") => {
            let b = parse_body::<newton_backend::PatchModuleDependencyBody>(body)?;
            store
                .patch_module_dependency(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Delete, "module-dependency" | "module-dependencies") => store
            .delete_module_dependency(id)
            .await
            .map_err(api_err)
            .and_then(|deleted_id| to_json(serde_json::json!({"id": deleted_id}))),
        (DataVerb::Get, "kpis") => store.list_kpis().await.map_err(api_err).and_then(to_json),
        (DataVerb::Get, "kpi") => store.get_kpi(id).await.map_err(api_err).and_then(to_json),
        (DataVerb::Post, "kpi" | "kpis") => {
            let b = parse_body::<newton_backend::CreateKpiBody>(body)?;
            store.create_kpi(b).await.map_err(api_err).and_then(to_json)
        }
        (DataVerb::Get, "eval-runs") => store
            .list_eval_runs(
                eval_scope.map(str::to_string),
                eval_scope_id.map(str::to_string),
                eval_source.map(str::to_string),
                eval_limit,
            )
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "eval-run") => store
            .get_eval_run(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "eval-run" | "eval-runs") => {
            let b = parse_body::<newton_backend::CreateEvalRunBody>(body)?;
            store
                .create_eval_run(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Get, "grades") => store
            .list_grades(
                grade_run_id.map(str::to_string),
                grade_kpi_id.map(str::to_string),
            )
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "grade") => store.get_grade(id).await.map_err(api_err).and_then(to_json),
        (DataVerb::Post, "grade" | "grades") => {
            let b = parse_body::<newton_backend::CreateGradeBody>(body)?;
            store
                .create_grade(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Get, "findings") => store
            .list_findings(None, None, None)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "finding") => store
            .get_finding(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "finding" | "findings") => {
            let b = parse_body::<newton_backend::CreateFindingBody>(body)?;
            store
                .create_finding(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "finding" | "findings") => {
            let b = parse_body::<newton_backend::PatchFindingBody>(body)?;
            store
                .patch_finding(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Get, "change-requests") => store
            .list_change_requests(None)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "change-request") => store
            .get_change_request(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "change-request" | "change-requests") => {
            let b = parse_body::<newton_backend::CreateChangeRequestBody>(body)?;
            store
                .create_change_request(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "change-request" | "change-requests") => {
            let b = parse_body::<newton_backend::PatchChangeRequestBody>(body)?;
            store
                .patch_change_request(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Get, "plans") => store
            .list_plans(None, None, None)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "plan") => store.get_plan(id).await.map_err(api_err).and_then(to_json),
        (DataVerb::Post, "plan" | "plans") => {
            let b = parse_body::<newton_backend::CreatePlanBody>(body)?;
            store
                .create_plan(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "plan" | "plans") => {
            let b = parse_body::<newton_backend::PatchPlanBody>(body)?;
            store
                .patch_plan(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Get, "optimize-runs") => store
            .list_optimize_runs()
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Get, "optimize-run") => store
            .get_optimize_run(id)
            .await
            .map_err(api_err)
            .and_then(to_json),
        (DataVerb::Post, "optimize-run" | "optimize-runs") => {
            let b = parse_body::<newton_backend::CreateOptimizeRunBody>(body)?;
            store
                .create_optimize_run(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Patch, "optimize-run" | "optimize-runs") => {
            let b = parse_body::<newton_backend::PatchOptimizeRunBody>(body)?;
            store
                .patch_optimize_run(id, b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Get, "optimize-cycles") => {
            let run_id = args.run_id.as_deref().unwrap_or(id);
            store
                .list_optimize_cycles(run_id)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (DataVerb::Get, "optimize-cycle") => {
            // For single cycle GET we reuse get_optimize_run by run_id + trajectory; use list and filter by id
            Err("use 'optimize-cycles --run-id <run_id>' to list cycles; single cycle GET not supported".to_string())
        }
        (DataVerb::Post, "optimize-cycle" | "optimize-cycles") => {
            let b = parse_body::<newton_backend::CreateOptimizeCycleBody>(body)?;
            store
                .create_optimize_cycle(b)
                .await
                .map_err(api_err)
                .and_then(to_json)
        }
        (v, r) => Err(format!("unsupported combination: {v} {r}")),
    }
}

/// Direct (in-process) unit tests for `data()`'s `CliExit` error-return paths
/// (spec 074, PR-1). These call `data()` itself rather than spawning a
/// subprocess so they are reliably attributed by coverage tooling — mirrors
/// `mcp_data_malformed_call_no_exit.rs`'s in-process dispatch seam, one level
/// closer to the handler.
#[cfg(test)]
mod cli_exit_path_tests {
    use super::*;
    use crate::cli::args::DataVerb;
    use tempfile::TempDir;

    /// A workspace whose `.newton/state/` directory already exists, so
    /// `SqliteBackendStore::new` (which opens with `mode=rwc` but does not
    /// create missing *directories*, only the db file itself) can open
    /// `backend.sqlite` and run migrations. Mirrors
    /// `mcp_data_malformed_call_no_exit.rs::setup_workspace_with_db`.
    fn setup_workspace() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join(".newton/state")).expect("create state dir");
        dir
    }

    fn base_args(ws: &TempDir, verb: DataVerb, resource: &str) -> DataArgs {
        DataArgs {
            verb,
            resource: resource.to_string(),
            id: None,
            file: None,
            body: None,
            json: false,
            dry_run: false,
            workspace: Some(ws.path().to_path_buf()),
            state_dir: None,
            run_id: None,
            kpi_id: None,
            scope: None,
            scope_id: None,
            source: None,
            limit: None,
        }
    }

    /// Downcasts the `anyhow::Error` returned by `data()` to the `CliExit`
    /// every one of its error-return paths constructs.
    fn expect_cli_exit(err: anyhow::Error) -> CliExit {
        err.downcast::<CliExit>()
            .unwrap_or_else(|e| panic!("expected a CliExit, got: {e}"))
    }

    async fn seed_eval_run(ws: &TempDir, run_id: &str) {
        let state_dir = crate::cli::workspace_paths::resolve_state_dir(ws.path(), None);
        let workspace_paths = WorkspacePaths::with_state_dir(ws.path().to_path_buf(), state_dir);
        let db_url = workspace_paths.backend_sqlite_url();
        let store = newton_backend::SqliteBackendStore::new(&db_url)
            .await
            .expect("open store to seed eval-run");
        // create_eval_run validates its scope FK, so a real Product must
        // exist for scope="product" to be accepted.
        let product = store
            .create_product(newton_backend::CreateProductBody {
                name: "seed-product".to_string(),
            })
            .await
            .expect("seed product for eval-run scope FK");
        store
            .create_eval_run(newton_backend::CreateEvalRunBody {
                id: run_id.to_string(),
                source: "test".to_string(),
                scope: "product".to_string(),
                scope_id: product.id,
                score: None,
                verdict: None,
                summary: None,
                evaluated_at: None,
                grades: None,
                raw_assessment: None,
            })
            .await
            .expect("seed eval-run");
    }

    #[tokio::test]
    async fn data_001_file_and_body_are_mutually_exclusive() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "product");
        args.file = Some(ws.path().join("body.json"));
        args.body = Some("{}".to_string());

        let err = data(args).await.expect_err("must fail");
        let exit = expect_cli_exit(err);
        assert_eq!(exit.code, 1);
        assert!(exit.message.contains("DATA-001"), "{}", exit.message);
    }

    #[tokio::test]
    async fn store_open_failure_surfaces_as_cli_exit() {
        // No `.newton/state` dir created, and an explicit --state-dir that
        // does not exist either — SqliteBackendStore::new does not create
        // missing parent directories, only the db file itself, so opening it
        // must fail.
        let ws = tempfile::tempdir().expect("tempdir");
        let mut args = base_args(&ws, DataVerb::Get, "products");
        args.state_dir = Some(ws.path().join("does-not-exist").join("nested"));

        let err = data(args).await.expect_err("store open must fail");
        let exit = expect_cli_exit(err);
        assert_eq!(exit.code, 1);
        assert!(
            exit.message.contains("Failed to open backend store"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn data_004_invalid_json_in_file() {
        let ws = setup_workspace();
        let file_path = ws.path().join("bad.json");
        std::fs::write(&file_path, b"{not valid json").expect("write bad json");
        let mut args = base_args(&ws, DataVerb::Post, "product");
        args.file = Some(file_path);

        let err = data(args).await.expect_err("must fail");
        let exit = expect_cli_exit(err);
        assert!(exit.message.contains("DATA-004"), "{}", exit.message);
    }

    #[tokio::test]
    async fn data_006_run_id_rejected_for_unsupported_resource() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Get, "products");
        args.run_id = Some("run-1".to_string());

        let err = data(args).await.expect_err("must fail");
        let exit = expect_cli_exit(err);
        assert!(exit.message.contains("DATA-006"), "{}", exit.message);
    }

    #[tokio::test]
    async fn data_008_scope_rejected_for_unsupported_resource() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Get, "products");
        args.scope = Some("product".to_string());

        let err = data(args).await.expect_err("must fail");
        let exit = expect_cli_exit(err);
        assert!(exit.message.contains("DATA-008"), "{}", exit.message);
    }

    #[tokio::test]
    async fn data_005_missing_body_for_post() {
        let ws = setup_workspace();
        let args = base_args(&ws, DataVerb::Post, "product");

        let err = data(args).await.expect_err("must fail");
        let exit = expect_cli_exit(err);
        assert!(exit.message.contains("DATA-005"), "{}", exit.message);
    }

    #[tokio::test]
    async fn dry_run_component_rejects_missing_product_fk() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "component");
        args.dry_run = true;
        args.body = Some(serde_json::json!({"productId": "ghost-product"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("FK validation failed") && exit.message.contains("ghost-product"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn dry_run_repo_rejects_missing_component_fk() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "repo");
        args.dry_run = true;
        args.body = Some(serde_json::json!({"componentId": "ghost-component"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("FK validation failed")
                && exit.message.contains("ghost-component"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn dry_run_module_rejects_missing_repo_fk() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "module");
        args.dry_run = true;
        args.body = Some(serde_json::json!({"repoId": "ghost-repo"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("FK validation failed") && exit.message.contains("ghost-repo"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn dry_run_eval_run_requires_scope_and_scope_id() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "eval-run");
        args.dry_run = true;
        args.body = Some(serde_json::json!({"source": "dk-review"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("scope and scopeId are required"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn dry_run_eval_run_rejects_missing_scope_target() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "eval-run");
        args.dry_run = true;
        args.body =
            Some(serde_json::json!({"scope": "product", "scopeId": "ghost-product"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("FK validation failed") && exit.message.contains("ghost-product"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn dry_run_grade_requires_run_id() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "grade");
        args.dry_run = true;
        args.body = Some(serde_json::json!({"dimension": "tests"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("runId is required"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn dry_run_grade_rejects_missing_run_id_fk() {
        let ws = setup_workspace();
        let mut args = base_args(&ws, DataVerb::Post, "grade");
        args.dry_run = true;
        args.body = Some(serde_json::json!({"runId": "ghost-run"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("FK validation failed") && exit.message.contains("ghost-run"),
            "{}",
            exit.message
        );
    }

    #[tokio::test]
    async fn dry_run_grade_rejects_missing_kpi_id_fk() {
        let ws = setup_workspace();
        seed_eval_run(&ws, "real-run-1").await;
        let mut args = base_args(&ws, DataVerb::Post, "grade");
        args.dry_run = true;
        args.body =
            Some(serde_json::json!({"runId": "real-run-1", "kpiId": "ghost-kpi"}).to_string());

        let err = data(args).await.expect_err("dry-run FK check must fail");
        let exit = expect_cli_exit(err);
        assert!(
            exit.message.contains("FK validation failed") && exit.message.contains("ghost-kpi"),
            "{}",
            exit.message
        );
    }
}
