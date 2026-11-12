"""Test stand-ins for refs_test.sh (not shipped in the image).

cgi  <backend> <root> <portfile> <dir>   plays nginx + fcgiwrap: HTTP in, backend.sh as CGI.
auth <portfile> <dir>                     plays the runner's /auth/git, behind a real nginx.

Both record checkpoint events POSTed to /events in <dir>/events, and work-branch pushes POSTed to
/git/pushed in <dir>/pushed (the Authorization header, then the body); auth also logs each of its calls
in <dir>/auth-calls. The stub identity is the X-Stub-User header: a -> session-a, b -> session-b,
r -> runner, "deny" -> 401, anything else is passed on as it is, no header -> no identity.
A Basic password, when there is one, is the stub user instead, the way the runner reads a token.
"""
import base64
import http.server
import subprocess
import sys

STUB = {"a": "session-a", "b": "session-b", "r": "runner"}
MODE = sys.argv[1]


def identity(headers):
    """(status, user): 401 for "deny", else 200 and the user ("" when there is none)."""
    user = headers.get("X-Stub-User")
    auth = headers.get("Authorization", "")
    if auth.startswith("Basic "):
        user = base64.b64decode(auth[6:]).decode().partition(":")[2]
    if user == "deny":
        return 401, ""
    return 200, STUB.get(user, user) if user is not None else ""


class Handler(http.server.BaseHTTPRequestHandler):
    def body(self):
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            with open(DIR + "/chunked", "a") as f:
                f.write(self.path + "\n")
            data = b""
            while (size := int(self.rfile.readline().split(b";")[0], 16)) > 0:
                data += self.rfile.read(size)
                self.rfile.readline()
            self.rfile.readline()
            return data, ""
        data = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        return data, self.headers.get("Content-Length", "")

    def answer(self, status, headers, data):
        self.send_response(status)
        for key, value in headers:
            self.send_header(key, value)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def run(self):
        data, length = self.body()
        path, _, query = self.path.partition("?")
        if path == "/events":
            with open(DIR + "/events", "ab") as f:
                f.write(data + b"\n")
            return self.answer(204, [], b"")
        if path == "/git/pushed":
            with open(DIR + "/pushed", "ab") as f:
                f.write(self.headers.get("Authorization", "").encode() + b" " + data + b"\n")
            return self.answer(202, [], b"")
        status, user = identity(self.headers)
        if MODE == "auth":
            with open(DIR + "/auth-calls", "a") as f:
                f.write(path + "\n")
            return self.answer(status, [("X-Remote-User", user)] if user else [], b"")
        if status != 200:
            return self.answer(status, [], b"unauthorized\n")
        self.cgi(path, query, data, length, user)

    def cgi(self, path, query, data, length, user):
        # What nginx passes: the fastcgi params (REMOTE_USER empty without an identity,
        # CONTENT_LENGTH empty for a chunked body) and the request headers as HTTP_*.
        env = {"GATEWAY_INTERFACE": "CGI/1.1", "REQUEST_METHOD": self.command, "PATH_INFO": path,
               "QUERY_STRING": query, "CONTENT_TYPE": self.headers.get("Content-Type", ""),
               "CONTENT_LENGTH": length, "REMOTE_USER": user, "GIT_PROJECT_ROOT": ROOT,
               "GIT_HTTP_EXPORT_ALL": "1",
               "RANGELIGHT_EVENTS_URL": "http://127.0.0.1:%d/events" % self.server.server_port,
               "RANGELIGHT_PUSHED_URL": "http://127.0.0.1:%d/git/pushed" % self.server.server_port}
        for key, value in self.headers.items():
            if key.lower() not in ("content-type", "content-length", "x-stub-user"):
                env["HTTP_" + key.upper().replace("-", "_")] = value
        out = subprocess.run([BACKEND], input=data, env=env, stdout=subprocess.PIPE).stdout
        head, _, rest = out.partition(b"\r\n\r\n")
        status, headers = 200, []
        for line in head.decode().split("\r\n"):
            key, _, value = line.partition(":")
            if key.lower() == "status":
                status = int(value.split()[0])
            elif key:
                headers.append((key, value.strip()))
        self.answer(status, headers, rest)

    do_GET = do_POST = do_DELETE = run

    def log_message(self, *args):
        pass


if MODE == "cgi":
    BACKEND, ROOT, PORTFILE, DIR = sys.argv[2:6]
else:
    PORTFILE, DIR = sys.argv[2:4]
server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
with open(PORTFILE, "w") as f:
    f.write(str(server.server_port))
server.serve_forever()
