"""Drive the adopted edge listener with Python's actual HTTP codec."""

import http.client
import json
import socket
import sys

FORECAST = "/api/v1/sites/ballydidean/forecast"
CURRENT = "/api/v1/sites/ballydidean/current"
CURRENT_CACHE = "public, max-age=15, s-maxage=30, stale-while-revalidate=30"
WIDGET = "/api/v1/sites/ballydidean/widget-forecast"
RECORDED = "/api/v1/sites/ballydidean/forecast-observations"


class RequestBuffer(http.client.HTTPConnection):
    """Capture standard library request serialization for one pipelined write."""

    def __init__(self):
        super().__init__("127.0.0.1")
        self.data = bytearray()

    def send(self, data):
        """Retain request bytes instead of opening another HTTP socket."""
        self.data.extend(data)


class SharedReader:
    """Keep the same buffered reader across independent HTTPResponse objects."""

    def __init__(self, reader):
        self.reader = reader

    def __getattr__(self, attr):
        return getattr(self.reader, attr)

    def close(self):
        """Let only the owning socket close the shared response reader."""


class SharedSocket:
    """Borrow the existing reader for the standard response decoder."""

    def __init__(self, reader):
        self.reader = reader

    def makefile(self, *args, **kwargs):
        return SharedReader(self.reader)


def request_bytes(method, path, close=False):
    """Serialize each ordinary request using the standard library."""
    capture = RequestBuffer()
    headers = {"accept": "application/json, text/plain"}
    if close:
        headers["connection"] = "close"
    capture.request(method, path, headers=headers)
    return bytes(capture.data)


def mixed(port):
    """Read mixed ordered HEAD and GET responses over one pipelined socket."""
    cases = [
        ("HEAD", FORECAST, FORECAST, "no-store"),
        ("GET", FORECAST + "?days=%31", FORECAST, "no-store"),
        ("GET", WIDGET + "?", WIDGET, "no-store"),
        ("GET", RECORDED + "?", RECORDED, "public, max-age=60, s-maxage=60"),
        ("GET", CURRENT.replace("/current", "/unused/../current"), CURRENT, CURRENT_CACHE),
        ("GET", FORECAST + "?days=5", FORECAST, "no-store"),
        ("HEAD", FORECAST, FORECAST, "no-store"),
    ]
    with socket.create_connection(("127.0.0.1", port), timeout=5) as sock:
        # queue all requests before reading any response
        sock.sendall(b"".join(
            request_bytes(method, target, index == len(cases) - 1)
            for index, (method, target, _, _) in enumerate(cases)
        ))
        with sock.makefile("rb") as reader:
            shared = SharedSocket(reader)
            for method, _, path, cache in cases:
                response = http.client.HTTPResponse(shared, method=method)
                response.begin()
                # widget rejects a bare delimiter while recorded products accept empty search
                if path == WIDGET:
                    expected, status, content_type, vary = b"bad request\n", 400, "text/plain; charset=utf-8", None
                elif path == RECORDED:
                    expected, status, content_type, vary = b'{"data":[]}', 200, "application/json; charset=utf-8", None
                else:
                    expected = json.dumps({"path": path}, separators=(",", ":")).encode()
                    status, content_type, vary = 200, "application/json; charset=utf-8", "Accept"
                assert response.status == status
                assert response.getheader("content-length") == str(len(expected))
                assert response.getheader("content-type") == content_type
                assert response.getheader("cache-control") == cache
                assert response.getheader("vary") == vary
                assert response.read() == (b"" if method == "HEAD" else expected)
                response.close()


def close_mid_body(port, length):
    """Read the real final head and a small body prefix then disconnect."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        # the small receive window and larger projected fixture leave bytes unwritten
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4096)
        sock.settimeout(5)
        sock.connect(("127.0.0.1", port))
        sock.sendall(request_bytes("GET", FORECAST))
        response = http.client.HTTPResponse(sock)
        response.begin()
        assert response.status == 200
        assert response.getheader("content-length") == str(length)
        assert response.read(8) == b'{"path":'
        response.close()


if __name__ == "__main__":
    mode, port, length = sys.argv[1:]
    if mode == "mixed":
        mixed(int(port))
    else:
        close_mid_body(int(port), int(length))
