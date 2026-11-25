#!/usr/bin/env bash
# Serves the browser version built by scripts/build-web.sh (dist/web):
#
#   scripts/serve-web.sh [--https] [--port <n>]
#
#   (none)    http://localhost:8080, this machine only
#   --https   https://<this Mac>.local:8443 on the local network, for a phone or a tablet
#             (docs/ui/touch.md): WebGPU needs a secure page, and `localhost` is one only on the
#             machine itself
#   --port    another port (default 8080, 8443 with --https)
#
# --https takes a certificate from mkcert (`brew install mkcert`, then `mkcert -install` once:
# a local certificate authority in this Mac's keychain), made here for the machine's .local
# name, its address on the network and localhost, kept in target/serve-web/. A device trusts it
# once mkcert's root certificate is installed on it — on an iPhone or an iPad:
#   1. AirDrop it the file `rootCA.pem` of `mkcert -CAROOT` (this script names it);
#   2. Settings → Profile Downloaded → Install;
#   3. Settings → General → About → Certificate Trust Settings → full trust for « mkcert ».
# The pages are served with `Cache-Control: no-cache`: a build made again is taken on reload.
set -euo pipefail
cd "$(dirname "$0")/.."

https=false port=""
while [ $# -gt 0 ]; do
  case "$1" in
    --https) https=true ;;
    --port) port="${2:?--port needs a number}"; shift ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

[ -f dist/web/index.html ] || { echo "no dist/web: run scripts/build-web.sh first" >&2; exit 1; }

cert="" key="" bind="127.0.0.1"
if $https; then
  port="${port:-8443}"
  bind="0.0.0.0"
  command -v mkcert >/dev/null || {
    echo "mkcert is missing: brew install mkcert && mkcert -install" >&2
    exit 1
  }
  ca="$(mkcert -CAROOT)/rootCA.pem"
  [ -f "$ca" ] || { echo "mkcert has no authority yet: run mkcert -install" >&2; exit 1; }
  # This machine's names: its .local name (macOS), its address on the network (the default
  # route's interface), localhost.
  host="$(scutil --get LocalHostName 2>/dev/null || hostname -s).local"
  iface="$(route -n get default 2>/dev/null | awk '/interface:/ { print $2 }')"
  ip="$( { [ -n "$iface" ] && ipconfig getifaddr "$iface"; } 2>/dev/null || true)"
  names="$host${ip:+ $ip} localhost 127.0.0.1"
  dir=target/serve-web
  cert="$dir/cert.pem" key="$dir/key.pem"
  if [ ! -f "$cert" ] || [ "$(cat "$dir/names" 2>/dev/null)" != "$names" ]; then
    mkdir -p "$dir"
    # shellcheck disable=SC2086 # the names, one argument each
    mkcert -cert-file "$cert" -key-file "$key" $names
    echo "$names" > "$dir/names"
  fi
  echo "https://$host:$port${ip:+   (or https://$ip:$port)}"
  echo "on the device, once: AirDrop it $ca and trust it (scripts/serve-web.sh --help)"
else
  port="${port:-8080}"
  echo "http://localhost:$port"
fi

exec env SERVE_BIND="$bind" SERVE_PORT="$port" SERVE_CERT="$cert" SERVE_KEY="$key" python3 -I - <<'EOF'
import functools, http.server, os, ssl, sys

class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm", ".js": "text/javascript"}

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True
    tls = None

    # The TLS handshake in the connection's own thread, not in accept(): a browser opens
    # connections ahead and may send nothing on them, which would block every other request.
    def finish_request(self, request, client_address):
        if self.tls is None:
            return super().finish_request(request, client_address)
        request.settimeout(30)
        with self.tls.wrap_socket(request, server_side=True) as secure:
            secure.settimeout(None)
            super().finish_request(secure, client_address)

    # A browser drops connections it no longer needs (a request given up, a file it had): one
    # line, no traceback — a download that keeps being dropped is worth seeing.
    def handle_error(self, request, client_address):
        error = sys.exc_info()[1]
        if isinstance(error, (BrokenPipeError, ConnectionResetError, ssl.SSLError, TimeoutError)):
            sys.stderr.write(f"{client_address[0]} - dropped: {type(error).__name__}\n")
            return
        super().handle_error(request, client_address)

server = Server((os.environ["SERVE_BIND"], int(os.environ["SERVE_PORT"])), functools.partial(Handler, directory="dist/web"))
if os.environ["SERVE_CERT"]:
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(os.environ["SERVE_CERT"], os.environ["SERVE_KEY"])
    server.tls = context
try:
    server.serve_forever()
except KeyboardInterrupt:
    pass
EOF
