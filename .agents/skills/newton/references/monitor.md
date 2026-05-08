# newton monitor

## Purpose

Stream live ailoop channels for every project or branch in the workspace via a terminal UI. It surfaces blocking questions and authorizations in a queue and posts responses back to the ailoop HTTP API.

## Synopsis

```bash
newton monitor [OPTIONS]
```

## Configuration

Newton discovers the workspace root by walking up until it finds `.newton/`.

Monitor needs **both** HTTP and WebSocket URLs. Resolution order:

1. CLI: `--http-url` and `--ws-url` (partial overrides merge with config when the other value comes from a file).
2. `.newton/configs/monitor.conf` if present.
3. Other `.conf` files in `.newton/configs/`, sorted by filename, until both `ailoop_server_http_url` and `ailoop_server_ws_url` are set.

See [configuration.md](configuration.md) for key names and monitor-related keys. `ailoop_ws_url` alone is **not** used by Newton; set the server pair or pass CLI URLs.

Minimal `monitor.conf` example:

```
ailoop_server_http_url=http://127.0.0.1:8081
ailoop_server_ws_url=ws://127.0.0.1:8080
```

## Options

- `--http-url <URL>`: Override the ailoop HTTP base URL for this session.
- `--ws-url <URL>`: Override the ailoop WebSocket URL for this session.
- `--backend`: Also start Newton's HTTP API (`newton serve`) alongside the monitor (bind address and port: see `newton monitor --help`).

## Examples

### Basic usage with config file

```bash
# Start ailoop server first
ailoop serve

# Start monitor (reads from .newton/configs/monitor.conf)
newton monitor
```

### Override URLs

```bash
newton monitor --http-url http://127.0.0.1:8081 --ws-url ws://127.0.0.1:8080
```

### Full workflow

```bash
# Terminal 1: Start ailoop server
ailoop serve

# Terminal 2: Start newton monitor
newton monitor

# Terminal 3: Send messages
ailoop say "Task completed" --server ws://127.0.0.1:8080 --channel myproject
ailoop ask "Deploy to production?" --server ws://127.0.0.1:8080 --channel myproject
```

## UI

Default keybindings (may differ by Newton version; use `?` in the TUI for authoritative help):

- `/`: Filter messages
- `V`: Toggle layout (tiles or list)
- `Q`: Show queue-only view
- `?`: Show help
- `Esc`: Close dialogs or filters
- `↑` / `↓`: Navigate messages
- `Enter`: Respond to questions or authorizations

## Integration with ailoop

Newton monitor requires an [ailoop](https://github.com/goailoop/ailoop) server:

1. Install ailoop (for example `brew install ailoop` or `cargo install ailoop-cli`, per upstream docs).
2. Start the server: `ailoop serve`
3. Start the monitor: `newton monitor`

## Message protocol

Newton monitor expects messages in ailoop's Message format. Example shape:

```json
{
  "id": "<uuid>",
  "channel": "channel-name",
  "sender_type": "AGENT",
  "content": {
    "type": "notification",
    "text": "Message text",
    "priority": "normal"
  },
  "timestamp": "2024-01-15T10:00:00Z"
}
```

Supported content types (ailoop side; confirm against your ailoop version):

- `notification`: Simple messages
- `question`: Interactive questions with optional choices
- `authorization`: Authorization requests
- `response`: Responses to questions or authorizations
- `navigate`: URL navigation suggestions
- `workflow_progress`: Workflow status updates
- `task_create`, `task_update`: Task management

## Troubleshooting

**Monitor not receiving messages**

1. Enable debug logging, for example:
   ```bash
   RUST_LOG=newton=debug,info newton monitor
   ```
2. Check logs for subscription and parse lines (wording varies by build).
3. Verify ailoop is listening on the ports you configured (`lsof`, `ss`, or your OS equivalent).

**Parse errors in ailoop serve**

- Missing required fields such as `sender_type`
- Enum casing (for example use values ailoop expects for `sender_type`)
- Malformed JSON or wrong message schema

See the Newton and ailoop README troubleshooting sections for version-specific detail.
