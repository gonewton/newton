use newton::monitor::config::{load_monitor_endpoints, MonitorOverrides};
use std::fs;
use tempfile::TempDir;

#[test]
fn endpoint_resolution_http_override_ws_from_config() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(
        &monitor_conf,
        "ailoop_server_http_url = http://config.example.com:8081\n\
         ailoop_server_ws_url = ws://config.example.com:8080\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: Some("http://override.example.com:9081".to_string()),
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides).unwrap();
    assert_eq!(
        result.http_url.as_str(),
        "http://override.example.com:9081/"
    );
    assert_eq!(result.ws_url.as_str(), "ws://config.example.com:8080/");
}

#[test]
fn endpoint_resolution_ws_override_http_from_config() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(
        &monitor_conf,
        "ailoop_server_http_url = http://config.example.com:8081\n\
         ailoop_server_ws_url = ws://config.example.com:8080\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: Some("ws://override.example.com:9080".to_string()),
    };

    let result = load_monitor_endpoints(workspace.path(), overrides).unwrap();
    assert_eq!(result.http_url.as_str(), "http://config.example.com:8081/");
    assert_eq!(result.ws_url.as_str(), "ws://override.example.com:9080/");
}

#[test]
fn endpoint_resolution_missing_http_yields_specific_error() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(
        &monitor_conf,
        "ailoop_server_ws_url = ws://config.example.com:8080\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("HTTP endpoint"),
        "Error should mention missing HTTP endpoint: {}",
        error_msg
    );
    assert!(
        error_msg.contains("ailoop_server_http_url"),
        "Error should mention config key: {}",
        error_msg
    );
}

#[test]
fn endpoint_resolution_missing_ws_yields_specific_error() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(
        &monitor_conf,
        "ailoop_server_http_url = http://config.example.com:8081\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("WebSocket endpoint"),
        "Error should mention missing WebSocket endpoint: {}",
        error_msg
    );
    assert!(
        error_msg.contains("ailoop_server_ws_url"),
        "Error should mention config key: {}",
        error_msg
    );
}

#[test]
fn endpoint_resolution_missing_both_yields_specific_error() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(&monitor_conf, "# Empty config\n").unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("HTTP and WebSocket endpoints"),
        "Error should mention both missing endpoints: {}",
        error_msg
    );
}

#[test]
fn url_parse_failure_identifies_http_endpoint() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(
        &monitor_conf,
        "ailoop_server_http_url = not-a-valid-url\n\
         ailoop_server_ws_url = ws://config.example.com:8080\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("HTTP URL"),
        "Error should identify HTTP URL parse failure: {}",
        error_msg
    );
}

#[test]
fn url_parse_failure_identifies_ws_endpoint() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(
        &monitor_conf,
        "ailoop_server_http_url = http://config.example.com:8081\n\
         ailoop_server_ws_url = not-a-valid-url\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("WebSocket URL"),
        "Error should identify WebSocket URL parse failure: {}",
        error_msg
    );
}

#[test]
fn discovery_order_monitor_conf_takes_precedence() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    // Create monitor.conf
    let monitor_conf = configs_dir.join("monitor.conf");
    fs::write(
        &monitor_conf,
        "ailoop_server_http_url = http://monitor.example.com:8081\n\
         ailoop_server_ws_url = ws://monitor.example.com:8080\n",
    )
    .unwrap();

    // Create alphabetically-first .conf file
    let aaa_conf = configs_dir.join("aaa.conf");
    fs::write(
        &aaa_conf,
        "ailoop_server_http_url = http://aaa.example.com:8081\n\
         ailoop_server_ws_url = ws://aaa.example.com:8080\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides).unwrap();
    // monitor.conf should take precedence over aaa.conf
    assert_eq!(result.http_url.as_str(), "http://monitor.example.com:8081/");
    assert_eq!(result.ws_url.as_str(), "ws://monitor.example.com:8080/");
}

#[test]
fn discovery_order_alphabetical_fallback() {
    let workspace = TempDir::new().unwrap();
    let configs_dir = workspace.path().join(".newton").join("configs");
    fs::create_dir_all(&configs_dir).unwrap();

    // Create two .conf files (no monitor.conf)
    let bbb_conf = configs_dir.join("bbb.conf");
    fs::write(
        &bbb_conf,
        "ailoop_server_http_url = http://bbb.example.com:8081\n\
         ailoop_server_ws_url = ws://bbb.example.com:8080\n",
    )
    .unwrap();

    let zzz_conf = configs_dir.join("zzz.conf");
    fs::write(
        &zzz_conf,
        "ailoop_server_http_url = http://zzz.example.com:8081\n\
         ailoop_server_ws_url = ws://zzz.example.com:8080\n",
    )
    .unwrap();

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides).unwrap();
    // bbb.conf should be chosen (alphabetically first)
    assert_eq!(result.http_url.as_str(), "http://bbb.example.com:8081/");
    assert_eq!(result.ws_url.as_str(), "ws://bbb.example.com:8080/");
}

#[test]
fn missing_configs_dir_provides_actionable_error() {
    let workspace = TempDir::new().unwrap();
    // Don't create .newton/configs directory

    let overrides = MonitorOverrides {
        http_url: None,
        ws_url: None,
    };

    let result = load_monitor_endpoints(workspace.path(), overrides);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains(".newton/configs"),
        "Error should mention .newton/configs: {}",
        error_msg
    );
    assert!(
        error_msg.contains("newton init") || error_msg.contains("To fix"),
        "Error should provide remediation guidance: {}",
        error_msg
    );
}

#[test]
fn cli_overrides_work_without_config_dir() {
    let workspace = TempDir::new().unwrap();
    // Don't create .newton/configs directory - CLI overrides should still work

    let overrides = MonitorOverrides {
        http_url: Some("http://override.example.com:8081".to_string()),
        ws_url: Some("ws://override.example.com:8080".to_string()),
    };

    let result = load_monitor_endpoints(workspace.path(), overrides).unwrap();
    assert_eq!(
        result.http_url.as_str(),
        "http://override.example.com:8081/"
    );
    assert_eq!(result.ws_url.as_str(), "ws://override.example.com:8080/");
}
