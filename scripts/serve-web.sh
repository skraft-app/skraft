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
# The wasm, the scripts and the page go compressed (55 MB of wasm → 14), as a host serves them:
# brotli at its level 5 when its command is there (`brew install brotli`), else gzip; each file
# compressed on its first request and kept in target/serve-web/encoded/ while it is unchanged.
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

command -v brotli >/dev/null || echo "no brotli command: gzip instead (brew install brotli)"

exec env SERVE_BIND="$bind" SERVE_PORT="$port" SERVE_CERT="$cert" SERVE_KEY="$key" SERVE_CACHE=target/serve-web/encoded \
  python3 -I - <<'EOF'
import email.utils, functools, gzip, http.server, os, shutil, ssl, subprocess, sys, threading

ROOT = "dist/web"
CACHE = os.environ["SERVE_CACHE"]
BROTLI = shutil.which("brotli")
COMPRESSED = {".wasm", ".js", ".html", ".css", ".json", ".svg", ".txt"}
locks, locks_lock = {}, threading.Lock()

def encoded(path, stat, coding):
    """The file compressed with `coding`, made again when the file changed."""
    out = os.path.join(CACHE, os.path.relpath(path, ROOT) + "." + coding)
    stamp, want = out + ".stamp", f"{stat.st_mtime_ns} {stat.st_size}"
    with locks_lock:
        lock = locks.setdefault(out, threading.Lock())
    with lock:
        try:
            with open(stamp) as f:
                if f.read() == want and os.path.exists(out):
                    return out
        except OSError:
            pass
        os.makedirs(os.path.dirname(out), exist_ok=True)
        if coding == "br":
            data = subprocess.run([BROTLI, "-c", "-q", "5", path], check=True, capture_output=True).stdout
        else:
            with open(path, "rb") as f:
                data = gzip.compress(f.read(), 6)
        with open(out + ".part", "wb") as f:
            f.write(data)
        os.replace(out + ".part", out)
        with open(stamp, "w") as f:
            f.write(want)
        return out

class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm", ".js": "text/javascript"}

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def send_head(self):
        path = self.translate_path(self.path)
        accepted = self.headers.get("Accept-Encoding", "")
        coding = "br" if BROTLI and "br" in accepted else "gzip" if "gzip" in accepted else None
        if coding is None or os.path.splitext(path)[1] not in COMPRESSED or not os.path.isfile(path):
            return super().send_head()
        stat = os.stat(path)
        since = self.headers.get("If-Modified-Since")
        if since:
            try:
                if int(stat.st_mtime) <= email.utils.parsedate_to_datetime(since).timestamp():
                    self.send_response(304)
                    self.end_headers()
                    return None
            except (TypeError, ValueError, IndexError, OverflowError):
                pass
        f = open(encoded(path, stat, coding), "rb")
        self.send_response(200)
        self.send_header("Content-Type", self.guess_type(path))
        self.send_header("Content-Encoding", coding)
        self.send_header("Content-Length", str(os.fstat(f.fileno()).st_size))
        self.send_header("Last-Modified", self.date_time_string(stat.st_mtime))
        self.send_header("Vary", "Accept-Encoding")
        self.end_headers()
        return f

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

server = Server((os.environ["SERVE_BIND"], int(os.environ["SERVE_PORT"])), functools.partial(Handler, directory=ROOT))
if os.environ["SERVE_CERT"]:
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(os.environ["SERVE_CERT"], os.environ["SERVE_KEY"])
    server.tls = context
try:
    server.serve_forever()
except KeyboardInterrupt:
    pass
EOF
