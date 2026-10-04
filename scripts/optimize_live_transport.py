"""Observe a trial's gateway transport without storing credentials or payloads.

Only the child agent uses the disposable registry. The operator's configuration
is unchanged. This proves transport to the gateway, not its upstream placement.
"""

import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
from pathlib import Path
import shutil
import tempfile
import threading
from urllib.parse import urlsplit

from optimize_live_route import PRIVATE_NETWORKS


class GatewayTransport:
    """Per-trial forwarding proxy with payload-free evidence and private peers."""

    def __init__(self, registry, model):
        self.provider, self.model = model.split("/", 1)
        self.records = []
        self._lock = threading.Lock()
        self._temporary = tempfile.TemporaryDirectory(prefix="newton-pi-route-")
        self.agent_dir = Path(self._temporary.name)
        self.server = None
        self.thread = None
        try:
            data = json.loads(registry.read_text())
            self.endpoint = urlsplit(data["providers"][self.provider]["baseUrl"])
            for name in ("auth.json", "settings.json"):
                source = registry.parent / name
                if source.is_file():
                    shutil.copyfile(source, self.agent_dir / name)
                    (self.agent_dir / name).chmod(0o600)
            owner = self

            class Handler(BaseHTTPRequestHandler):
                def log_message(self, *_args):
                    pass  # Default access/error logs can expose request content.

                def do_POST(self):
                    owner._forward(self)

            self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            self.server.daemon_threads = True
            data["providers"][self.provider]["baseUrl"] = (
                f"http://127.0.0.1:{self.server.server_port}{self.endpoint.path}"
            )
            path = self.agent_dir / "models.json"
            path.write_text(json.dumps(data))
            path.chmod(0o600)
            self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
            self.thread.start()
        except Exception:
            if self.server is not None:
                self.server.server_close()
            self._temporary.cleanup()
            raise

    def _forward(self, handler):
        connection = None
        record = {"model": self.model, "completed": False, "response_bytes": 0}
        try:
            length = int(handler.headers.get("Content-Length", "0"))
            if not 0 < length <= 16 * 1024 * 1024:
                raise ValueError("unsupported request length")
            body = handler.rfile.read(length)
            payload = json.loads(body)
            if payload.get("model") != self.model:
                raise ValueError("unexpected model")
            allowed = {self.endpoint.path.rstrip("/") + suffix for suffix in (
                "/chat/completions", "/responses", "/messages"
            )}
            if handler.path not in allowed:
                raise ValueError("unexpected inference path")
            record["path"] = handler.path
            cls = http.client.HTTPSConnection if self.endpoint.scheme == "https" else http.client.HTTPConnection
            connection = cls(self.endpoint.hostname, self.endpoint.port, timeout=180)
            connection.connect()
            peer = ipaddress.ip_address(connection.sock.getpeername()[0])
            if not any(peer in network for network in PRIVATE_NETWORKS):
                raise ValueError("upstream peer is not private")
            record["peer_address"] = str(peer)
            # Hop-by-hop headers cannot describe the separately framed connection.
            excluded = {"host", "connection", "transfer-encoding", "content-length", "proxy-authorization", "proxy-connection", "te", "trailer", "upgrade", "keep-alive"}
            headers = {key: value for key, value in handler.headers.items() if key.lower() not in excluded}
            connection.request("POST", handler.path, body=body, headers=headers)
            response = connection.getresponse()
            record["http_status"] = response.status
            handler.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() not in excluded:
                    handler.send_header(key, value)
            handler.send_header("Connection", "close")
            handler.end_headers()
            while chunk := response.read1(64 * 1024):
                handler.wfile.write(chunk)
                handler.wfile.flush()
                record["response_bytes"] += len(chunk)
            record["completed"] = 200 <= response.status < 300
        except Exception as error:
            # Do not retain exception messages: providers can echo credential data.
            record["error_type"] = type(error).__name__
            if "http_status" not in record:
                try:
                    handler.send_error(502, "gateway transport failed")
                except OSError:
                    pass
        finally:
            handler.close_connection = True
            if connection is not None:
                connection.close()
            with self._lock:
                self.records.append(record)

    def evidence(self):
        """Return redacted records, including failed or incomplete requests."""
        with self._lock:
            records = [dict(record) for record in self.records]
        observed = any(record["completed"] and record["response_bytes"] > 0 for record in records)
        return {
            "status": "observed_private_gateway_transport" if observed else "transport_not_observed",
            "transport_observed": observed,
            "endpoint_origin": f"{self.endpoint.scheme}://{self.endpoint.netloc}",
            "model": self.model,
            "requests": records,
            "upstream_model_locality": "unverified",
        }

    def close(self):
        """Stop accepting requests and erase the disposable agent configuration."""
        if self.server is not None:
            self.server.shutdown()
            self.server.server_close()
        if self.thread is not None:
            self.thread.join(timeout=5)
        self._temporary.cleanup()
