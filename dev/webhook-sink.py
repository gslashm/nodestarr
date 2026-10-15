"""Prints every request it receives, for testing notification channels.

Usage: python3 dev/webhook-sink.py [port]

Point a Webhook channel (or Discord, Slack, ntfy or Gotify, which take a
server or webhook address) at http://127.0.0.1:9099/anything and click
"Send a test".
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def handle_any(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0)).decode(errors="replace")
        try:
            body = json.dumps(json.loads(body), indent=2)
        except ValueError:
            pass
        print(f"--- {self.command} {self.path}  ({self.headers.get('Content-Type', '')})\n{body}\n", flush=True)
        reply = b'{"ok": true}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(reply)))
        self.end_headers()
        self.wfile.write(reply)

    do_POST = do_PUT = do_GET = handle_any

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9099
    print(f"Listening on http://127.0.0.1:{port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
