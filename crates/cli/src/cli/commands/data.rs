use crate::cli::args::{DataArgs, DataVerb};
use crate::cli::WorkspacePaths;
use newton_backend::BackendStore;
use std::fs;

pub async fn data(args: DataArgs) -> anyhow::Result<()> {
    if args.file.is_some() && args.body.is_some() {
        eprintln!("DATA-001: --file and --body are mutually exclusive; provide at most one");
        std::process::exit(1);
    }

    let workspace = match args.workspace {
        Some(ref p) => p.clone(),
        None => std::env::current_dir()?,
    };
    let workspace_paths = WorkspacePaths::new(workspace);
    let db_url = workspace_paths.backend_sqlite_url();
    let store = match newton_backend::SqliteBackendStore::new(&db_url).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open backend store: {}", e.message);
            std::process::exit(1);
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
                eprintln!("DATA-004: invalid JSON in body: {e}");
                std::process::exit(1);
            }
        }
    } else if let Some(ref s) = args.body {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("DATA-004: invalid JSON in --body: {e}");
                std::process::exit(1);
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
        eprintln!(
            "DATA-006: --run-id/--kpi-id are only supported with: resource=grades, optimize-cycles"
        );
        std::process::exit(1);
    }
    if (args.scope.is_some()
        || args.scope_id.is_some()
        || args.source.is_some()
        || args.limit.is_some())
        && resource != "eval-runs"
    {
        eprintln!(
            "DATA-008: --scope/--scope-id/--source/--limit are only supported with: resource=eval-runs"
        );
        std::process::exit(1);
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
        eprintln!("DATA-003: unknown resource '{resource}'; must be one of: product, products, component, components, repo, repos, module, modules, module-dependency, module-dependencies, kpi, kpis, eval-run, eval-runs, grade, grades, finding, findings, change-request, change-requests, plan, plans, optimize-run, optimize-runs, optimize-cycle, optimize-cycles");
        std::process::exit(1);
    }

    if matches!(args.verb, DataVerb::Post | DataVerb::Put | DataVerb::Patch) && body_value.is_none()
    {
        eprintln!("DATA-005: --file or --body is required for {}", args.verb);
        std::process::exit(1);
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
        eprintln!("DATA-002: ID is required for {} {}", args.verb, resource);
        std::process::exit(1);
    }

    if args.dry_run {
        if matches!(args.verb, DataVerb::Post | DataVerb::Put | DataVerb::Patch) {
            if let Some(ref v) = body_value {
                match resource {
                    "component" | "components" => {
                        if let Some(product_id) = v.get("productId").and_then(|p| p.as_str()) {
                            if let Err(e) = store.get_product(product_id).await {
                                eprintln!(
                                    "[dry-run] FK validation failed: productId '{}' not found: {}",
                                    product_id, e.message
                                );
                                std::process::exit(1);
                            }
                        }
                    }
                    "repo" | "repos" => {
                        if let Some(component_id) = v.get("componentId").and_then(|c| c.as_str()) {
                            if let Err(e) = store.get_component(component_id).await {
                                eprintln!("[dry-run] FK validation failed: componentId '{}' not found: {}", component_id, e.message);
                                std::process::exit(1);
                            }
                        }
                    }
                    "module" | "modules" => {
                        if let Some(repo_id) = v.get("repoId").and_then(|r| r.as_str()) {
                            if let Err(e) = store.get_repo(repo_id).await {
                                eprintln!(
                                    "[dry-run] FK validation failed: repoId '{}' not found: {}",
                                    repo_id, e.message
                                );
                                std::process::exit(1);
                            }
                        }
                    }
                    "eval-run" | "eval-runs" => {
                        let scope = v.get("scope").and_then(|s| s.as_str()).unwrap_or("");
                        let scope_id = v.get("scopeId").and_then(|s| s.as_str()).unwrap_or("");
                        if scope.is_empty() || scope_id.is_empty() {
                            eprintln!(
                                "[dry-run] FK validation failed: scope and scopeId are required"
                            );
                            std::process::exit(1);
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
                            eprintln!(
                                "[dry-run] FK validation failed: {} '{}' not found: {}",
                                scope, scope_id, e.message
                            );
                            std::process::exit(1);
                        }
                    }
                    "grade" | "grades" => {
                        let run_id = v.get("runId").and_then(|r| r.as_str());
                        let Some(run_id) = run_id else {
                            eprintln!("[dry-run] FK validation failed: runId is required");
                            std::process::exit(1);
                        };
                        if let Err(e) = store.get_eval_run(run_id).await {
                            eprintln!(
                                "[dry-run] FK validation failed: runId '{}' not found: {}",
                                run_id, e.message
                            );
                            std::process::exit(1);
                        }
                        if let Some(kpi_id) = v.get("kpiId").and_then(|k| k.as_str()) {
                            if let Err(e) = store.get_kpi(kpi_id).await {
                                eprintln!(
                                    "[dry-run] FK validation failed: kpiId '{}' not found: {}",
                                    kpi_id, e.message
                                );
                                std::process::exit(1);
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
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
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
