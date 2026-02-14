# `newton monitor`

Stream live ailoop channels for every project/branch in the workspace via a terminal UI.

## Synopsis

```bash
newton monitor [OPTIONS]
```

## Description

`newton monitor` connects to an ailoop server and displays real-time messages from all channels in a terminal UI. It highlights blocking questions and authorizations in a queue panel, allowing you to answer/approve/deny directly from the terminal.

The monitor discovers workspace configuration from `.newton/configs/` and looks for `ailoop_server_http_url` and `ailoop_server_ws_url` settings.

## Options

- `--http-url <URL>`: Override the HTTP base URL for this session
- `--ws-url <URL>`: Override the WebSocket URL for this session

## Configuration

Create `.newton/configs/monitor.conf` to avoid passing URLs each time:

```
ailoop_server_http_url=http://127.0.0.1:8081
ailoop_server_ws_url=ws://127.0.0.1:8080
```

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

## UI Controls

- `/`: Filter messages
- `V`: Toggle layout (tiles/list)
- `Q`: Show queue-only view
- `?`: Show help
- `Esc`: Close dialogs/filters
- `↑/↓`: Navigate messages
- `Enter`: Respond to questions/authorizations

## Integration with ailoop

Newton monitor requires an [ailoop](https://github.com/goailoop/ailoop) server running:

1. Install ailoop: `brew install ailoop` or `cargo install ailoop-cli`
2. Start server: `ailoop serve`
3. Start monitor: `newton monitor`

Messages sent to ailoop will appear in real-time in the newton monitor UI.

## Message Protocol

Newton monitor expects messages in ailoop's Message format:

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

Supported content types:
- `notification`: Simple messages
- `question`: Interactive questions with optional choices
- `authorization`: Authorization requests
- `response`: Responses to questions/authorizations
- `navigate`: URL navigation suggestions
- `workflow_progress`: Workflow status updates
- `task_create`, `task_update`: Task management

## Troubleshooting

**Monitor not receiving messages:**

1. Enable debug logging:
   ```bash
   RUST_LOG=newton=debug,info newton monitor
   ```

2. Check for "Subscription message sent successfully" and "Parsed message" logs

3. Verify ailoop server is listening:
   ```bash
   lsof -i :8080 -i :8081
   ```

**Parse errors in ailoop serve:**

- `missing field 'sender_type'`: Message missing required fields
- `unknown variant 'agent'`: Use uppercase "AGENT" or "HUMAN" for sender_type
- `expected struct Message`: Incorrect message structure

See README.md Troubleshooting section for more details.
