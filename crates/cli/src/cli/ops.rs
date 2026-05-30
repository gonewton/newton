//! Operational/diagnostic commands required by the org-baseline CLI checklist:
//! `health`, `doctor`, `config show`, `completion`.
//!
//! These commands MUST be runnable without a configured workspace.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

use crate::cli::WorkspacePaths;

pub mod error_codes {
    pub const CLI_OPS_001: &str = "CLI-OPS-001";
    pub const CLI_OPS_002: &str = "CLI-OPS-002";
    pub const CLI_OPS_003: &str = "CLI-OPS-003";
    pub const CLI_OPS_004: &str = "CLI-OPS-004";
    pub const CLI_OPS_006: &str = "CLI-OPS-006";
}

// ── health ───────────────────────────────────────────────────────────────────

pub mod health {
    use super::*;

    /// Print a single liveness line and exit 0.
    pub fn run() -> Result<()> {
        run_with_version(crate::VERSION)
    }

    pub fn run_with_version(version: &str) -> Result<()> {
        if version.is_empty() {
            return Err(anyhow!("{}: VERSION is empty", error_codes::CLI_OPS_001));
        }
        println!("newton OK {version}");
        Ok(())
    }
}

// ── doctor ───────────────────────────────────────────────────────────────────

pub mod doctor {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ProbeStatus {
        Ok,
        Fail,
        Skip,
    }

    impl ProbeStatus {
        fn label(self) -> &'static str {
            match self {
                ProbeStatus::Ok => "OK",
                ProbeStatus::Fail => "FAIL",
                ProbeStatus::Skip => "SKIP",
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct Probe {
        pub name: String,
        pub status: ProbeStatus,
        pub detail: String,
    }

    #[derive(Debug, Clone, Default)]
    pub struct DoctorReport {
        pub probes: Vec<Probe>,
    }

    impl DoctorReport {
        pub fn any_failed(&self) -> bool {
            self.probes.iter().any(|p| p.status == ProbeStatus::Fail)
        }

        pub fn print(&self) {
            for p in &self.probes {
                println!("{} {}: {}", p.status.label(), p.name, p.detail);
            }
        }
    }

    #[derive(Debug, Clone, Default)]
    pub struct DoctorArgs {
        pub workspace: Option<PathBuf>,
    }

    pub fn run(args: DoctorArgs) -> Result<DoctorReport> {
        let mut report = DoctorReport::default();

        report.probes.push(Probe {
            name: "version".into(),
            status: ProbeStatus::Ok,
            detail: crate::VERSION.to_string(),
        });

        // Workspace probe
        let ws_candidate = args.workspace.clone().or_else(|| {
            std::env::current_dir()
                .ok()
                .filter(|cwd| cwd.join(".newton").is_dir())
        });
        match ws_candidate {
            Some(ws) => match probe_workspace_writable(&ws) {
                Ok(()) => report.probes.push(Probe {
                    name: "workspace".into(),
                    status: ProbeStatus::Ok,
                    detail: ws.display().to_string(),
                }),
                Err(e) => report.probes.push(Probe {
                    name: "workspace".into(),
                    status: ProbeStatus::Fail,
                    detail: format!("{}: {}", error_codes::CLI_OPS_002, e),
                }),
            },
            None => report.probes.push(Probe {
                name: "workspace".into(),
                status: ProbeStatus::Skip,
                detail: "no .newton/ in CWD and --workspace not set".into(),
            }),
        }

        // Config probe
        let monitor_conf = args
            .workspace
            .as_ref()
            .map(|w| w.join(".newton/configs/monitor.conf"));
        let monitor_conf_text = match &monitor_conf {
            Some(p) if p.exists() => std::fs::read_to_string(p).ok(),
            _ => None,
        };
        match (&monitor_conf, &monitor_conf_text) {
            (Some(p), Some(_)) => report.probes.push(Probe {
                name: "config".into(),
                status: ProbeStatus::Ok,
                detail: p.display().to_string(),
            }),
            _ => report.probes.push(Probe {
                name: "config".into(),
                status: ProbeStatus::Skip,
                detail: "no monitor.conf found".into(),
            }),
        }

        // ailoop probe (HTTP health) — best-effort, only when URL resolvable
        let ailoop_url = monitor_conf_text.as_deref().and_then(parse_ailoop_http_url);
        match ailoop_url {
            Some(url) => report
                .probes
                .push(probe_ailoop(&url).unwrap_or_else(|e| Probe {
                    name: "ailoop".into(),
                    status: ProbeStatus::Fail,
                    detail: format!("{}: {}", error_codes::CLI_OPS_003, e),
                })),
            None => report.probes.push(Probe {
                name: "ailoop".into(),
                status: ProbeStatus::Skip,
                detail: "ailoop_server_http_url not configured".into(),
            }),
        }

        // gh probe
        let gh = which("gh");
        match gh {
            Some(p) => report.probes.push(Probe {
                name: "gh".into(),
                status: ProbeStatus::Ok,
                detail: p.display().to_string(),
            }),
            None => report.probes.push(Probe {
                name: "gh".into(),
                status: ProbeStatus::Skip,
                detail: "gh not on PATH".into(),
            }),
        }

        // logging probe — write a marker file in tempdir
        report.probes.push(probe_logging());

        Ok(report)
    }

    fn probe_workspace_writable(ws: &Path) -> std::io::Result<()> {
        let dot = ws.join(".newton");
        let probe = dot.join(".doctor-probe");
        std::fs::write(&probe, b"ok")?;
        let _ = std::fs::remove_file(&probe);
        Ok(())
    }

    fn parse_ailoop_http_url(text: &str) -> Option<String> {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("ailoop_server_http_url") {
                let rest = rest.trim_start_matches([' ', '\t']);
                if let Some(rest) = rest.strip_prefix('=') {
                    return Some(rest.trim().to_string());
                }
            }
        }
        None
    }

    fn probe_ailoop(url: &str) -> Result<Probe> {
        // Best-effort TCP reachability check; the existence of the URL is more
        // important than a successful HTTP exchange here.
        use std::net::ToSocketAddrs;
        let parsed = url::Url::parse(url).map_err(|e| anyhow!("invalid URL: {e}"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| anyhow!("URL has no host"))?;
        let port = parsed.port_or_known_default().unwrap_or(80);
        let addr = format!("{host}:{port}");
        let socket = addr
            .to_socket_addrs()
            .map_err(|e| anyhow!("resolve: {e}"))?
            .next()
            .ok_or_else(|| anyhow!("no address resolved"))?;
        match std::net::TcpStream::connect_timeout(&socket, Duration::from_secs(2)) {
            Ok(_) => Ok(Probe {
                name: "ailoop".into(),
                status: ProbeStatus::Ok,
                detail: format!("{addr} reachable"),
            }),
            Err(e) => Ok(Probe {
                name: "ailoop".into(),
                status: ProbeStatus::Fail,
                detail: format!("{}: {e}", error_codes::CLI_OPS_003),
            }),
        }
    }

    fn probe_logging() -> Probe {
        let tmp = std::env::temp_dir().join(format!("newton-doctor-{}.log", std::process::id()));
        match std::fs::write(&tmp, b"info doctor probe\n") {
            Ok(()) => {
                let exists = tmp.exists();
                let _ = std::fs::remove_file(&tmp);
                if exists {
                    Probe {
                        name: "logging".into(),
                        status: ProbeStatus::Ok,
                        detail: "tempdir writable".into(),
                    }
                } else {
                    Probe {
                        name: "logging".into(),
                        status: ProbeStatus::Fail,
                        detail: "wrote tempfile but it does not exist".into(),
                    }
                }
            }
            Err(e) => Probe {
                name: "logging".into(),
                status: ProbeStatus::Fail,
                detail: format!("{e}"),
            },
        }
    }

    fn which(binary: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(binary);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

// ── config show ──────────────────────────────────────────────────────────────

pub mod config_show {
    use super::*;

    #[derive(Debug, Clone, Default)]
    pub struct ConfigShowArgs {
        pub workspace: Option<PathBuf>,
    }

    pub fn run(args: ConfigShowArgs) -> Result<()> {
        let mut root = Map::new();
        root.insert("newton_version".into(), json!(crate::VERSION));

        // Resolve workspace paths — always, regardless of whether --workspace was given.
        let workspace_paths = match &args.workspace {
            Some(ws) => {
                if !ws.exists() {
                    return Err(anyhow!(
                        "{}: workspace '{}' does not exist",
                        error_codes::CLI_OPS_004,
                        ws.display()
                    ));
                }
                WorkspacePaths::new(ws.clone())
            }
            None => WorkspacePaths::from_cwd()
                .map_err(|e| anyhow!("{}: {e}", error_codes::CLI_OPS_006))?,
        };

        root.insert(
            "paths".into(),
            Value::Object(workspace_paths.to_json_object()),
        );

        let mut logging = Map::new();
        logging.insert(
            "log_dir".into(),
            json!(workspace_paths
                .dot_newton
                .join("logs")
                .display()
                .to_string()),
        );
        logging.insert("level".into(), json!(env_str("RUST_LOG", "info")));
        root.insert("logging".into(), Value::Object(logging));

        if workspace_paths.monitor_conf_exists() {
            if let Ok(text) = std::fs::read_to_string(&workspace_paths.monitor_conf) {
                let mut ailoop = Map::new();
                for (k, v) in parse_kv(&text) {
                    ailoop.insert(k.clone(), json!(redact_value(&k, &v)));
                }
                if !ailoop.is_empty() {
                    root.insert("ailoop".into(), Value::Object(ailoop));
                }
            }
        }

        // env-driven token entries always redacted
        let mut env_section = Map::new();
        for (k, v) in std::env::vars() {
            if k.starts_with("NEWTON_") {
                env_section.insert(k.clone(), json!(redact_value(&k, &v)));
            }
        }
        if !env_section.is_empty() {
            root.insert("env".into(), Value::Object(env_section));
        }

        let redacted = redact_object(Value::Object(root));
        let out = serde_json::to_string_pretty(&redacted)?;
        println!("{out}");
        Ok(())
    }

    fn env_str(name: &str, default: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| default.to_string())
    }

    fn parse_kv(text: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                out.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        out
    }

    fn redact_value(key: &str, value: &str) -> String {
        if is_secret_key(key) {
            "***REDACTED***".into()
        } else {
            value.to_string()
        }
    }

    pub(crate) fn is_secret_key(key: &str) -> bool {
        let lc = key.to_ascii_lowercase();
        lc.contains("token")
            || lc.contains("secret")
            || lc.contains("password")
            || lc.contains("key")
    }

    fn redact_object(v: Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut out = Map::new();
                for (k, child) in map {
                    let new_child = if is_secret_key(&k) {
                        match child {
                            Value::String(_) | Value::Number(_) | Value::Bool(_) => {
                                Value::String("***REDACTED***".into())
                            }
                            other => redact_object(other),
                        }
                    } else {
                        redact_object(child)
                    };
                    out.insert(k, new_child);
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.into_iter().map(redact_object).collect()),
            other => other,
        }
    }
}
