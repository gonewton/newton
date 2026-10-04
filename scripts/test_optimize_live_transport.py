"""Real loopback HTTP transport tests; no agent or remote provider is called."""

import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import tempfile
import threading
import unittest
from urllib.parse import urlsplit

from optimize_live_transport import GatewayTransport


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.status = 200
        self.response = b'data: {"private":"response-content"}\n\ndata: [DONE]\n\n'
        self.received = []
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def do_POST(self):
                body = self.rfile.read(int(self.headers["Content-Length"]))
                owner.received.append((json.loads(body), self.headers.get("Authorization")))
                self.send_response(owner.status)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(owner.response)))
                self.end_headers()
                self.wfile.write(owner.response)

        gateway = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.addCleanup(gateway.server_close)
        self.addCleanup(gateway.shutdown)
        threading.Thread(target=gateway.serve_forever, daemon=True).start()
        self.registry = self.root / "models.json"
        self.original = json.dumps({"providers": {"local": {
            "baseUrl": f"http://127.0.0.1:{gateway.server_port}/v1",
            "apiKey": "PRIVATE_CREDENTIAL",
            "models": [{"id": "coder"}],
        }}})
        self.registry.write_text(self.original)
        self.transport = GatewayTransport(self.registry, "local/coder")
        self.addCleanup(self.transport.close)
        data = json.loads((self.transport.agent_dir / "models.json").read_text())
        self.proxy = urlsplit(data["providers"]["local"]["baseUrl"])

    def invoke(self, model="coder"):
        connection = http.client.HTTPConnection(self.proxy.hostname, self.proxy.port, timeout=10)
        try:
            body = json.dumps({"model": model, "messages": [{"content": "PRIVATE_PROMPT"}], "stream": True})
            connection.request("POST", "/v1/chat/completions", body, {"Authorization": "Bearer PRIVATE_CREDENTIAL", "Content-Type": "application/json"})
            response = connection.getresponse()
            return response.status, response.read()
        finally:
            connection.close()

    def test_streamed_exchange_is_observed_without_payload_or_credential_retention(self):
        self.assertEqual(self.invoke(), (200, self.response))
        evidence = self.transport.evidence()
        self.assertTrue(evidence["transport_observed"])
        self.assertEqual(evidence["requests"][0]["peer_address"], "127.0.0.1")
        self.assertEqual(evidence["requests"][0]["response_bytes"], len(self.response))
        self.assertEqual(self.received[0][1], "Bearer PRIVATE_CREDENTIAL")
        rendered = json.dumps(evidence)
        for private in ("PRIVATE_CREDENTIAL", "PRIVATE_PROMPT", "response-content"):
            self.assertNotIn(private, rendered)
        self.assertEqual(self.registry.read_text(), self.original)

    def test_failed_or_empty_exchange_cannot_pass_the_transport_gate(self):
        self.status = 503
        self.invoke()
        self.assertFalse(self.transport.evidence()["transport_observed"])
        self.status = 200
        self.response = b""
        self.invoke()
        self.assertFalse(self.transport.evidence()["transport_observed"])
        self.assertEqual(self.invoke(model="unexpected-model")[0], 502)
        self.assertEqual(len(self.received), 2)
        self.assertFalse(self.transport.evidence()["transport_observed"])


if __name__ == "__main__":
    unittest.main()
