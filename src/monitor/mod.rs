//! Monitor command wiring: HTTP/WebSocket polling plus a ratatui terminal UI for observing ailoop channels and responding to events.
pub mod client;
pub mod config;
pub mod event;
pub mod message;
pub mod state;
pub mod ui;

use crate::cli::args::MonitorArgs;
use crate::core::batch_config::find_workspace_root;
use crate::monitor::client::{
    command_loop, initial_backfill, polling_loop, websocket_loop, AiloopClient,
};
use crate::monitor::config::{load_monitor_endpoints, MonitorOverrides};
use crate::Result;
use std::env;
use tokio::sync::mpsc::unbounded_channel;

/// Run the monitor command, wiring up the network layer and TUI.
pub async fn run(args: MonitorArgs) -> Result<()> {
    let current_dir = env::current_dir()?;
    let workspace_root = find_workspace_root(&current_dir)?;

    let overrides = MonitorOverrides {
        http_url: args.http_url,
        ws_url: args.ws_url,
    };

    let endpoints = load_monitor_endpoints(&workspace_root, overrides)?;

    let (event_tx, event_rx) = unbounded_channel();
    let (command_tx, command_rx) = unbounded_channel();

    let client = AiloopClient::new(endpoints.clone());
    if let Err(err) = initial_backfill(&client, &event_tx).await {
        tracing::warn!("monitor backfill failed: {}", err);
    }

    let ws_handle = tokio::spawn(websocket_loop(client.clone(), event_tx.clone()));
    let poll_handle = tokio::spawn(polling_loop(client.clone(), event_tx.clone()));
    let command_handle = tokio::spawn(command_loop(client.clone(), command_rx, event_tx.clone()));

    tokio::task::spawn_blocking(move || {
        ui::run_tui(endpoints, workspace_root, event_rx, command_tx)
    })
    .await??;

    ws_handle.abort();
    poll_handle.abort();
    command_handle.abort();

    Ok(())
}
