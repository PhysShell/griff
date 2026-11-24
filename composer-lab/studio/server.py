"""Local HTTP server for Studio."""
import argparse
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import sys
from pathlib import Path
from urllib.parse import urlparse

try:
    import msvcrt
except ImportError:
    msvcrt = None
    import fcntl

from studio.engine import StudioStore
from studio.render import default_render, fake_render

INDEX_HTML = Path(__file__).with_name("index.html")
MAX_BODY = 64 * 1024
MAX_DRAIN = 8 * 1024 * 1024
MAX_SEED = 10**15


class BodyTooLarge(Exception):
    pass


def _text_field(body, name):
    value = body.get(name)
    if not isinstance(value, str) or not value:
        raise ValueError(f"{name} must be a non-empty string")
    return value


def _make_handler(store):
    class StudioHandler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *args):
            return

        def _send_json(self, code, obj, download_name=None):
            body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            if download_name:
                self.send_header("Content-Disposition", f'attachment; filename="{download_name}"')
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(body)

        def _send_file(self, path, content_type, download_name=None, support_range=False):
            data = path.read_bytes()
            total = len(data)
            rng = self.headers.get("Range") if support_range else None
            span = None
            if rng and rng.startswith("bytes="):
                a_s, _, b_s = rng[6:].split(",", 1)[0].strip().partition("-")
                try:
                    span = (int(a_s) if a_s else 0, int(b_s) if b_s else total - 1)
                except ValueError:
                    span = None
            if span is not None:
                start, end = span
                if start >= total or start < 0 or end < start:
                    self.send_response(HTTPStatus.REQUESTED_RANGE_NOT_SATISFIABLE)
                    self.send_header("Content-Range", f"bytes */{total}")
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                end = min(end, total - 1)
                chunk = data[start:end + 1]
                self.send_response(HTTPStatus.PARTIAL_CONTENT)
                self.send_header("Content-Type", content_type)
                self.send_header("Accept-Ranges", "bytes")
                self.send_header("Content-Range", f"bytes {start}-{end}/{total}")
                self.send_header("Content-Length", str(len(chunk)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(chunk)
                return

            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", content_type)
            if support_range:
                self.send_header("Accept-Ranges", "bytes")
            if download_name:
                self.send_header("Content-Disposition", f'attachment; filename="{download_name}"')
            self.send_header("Content-Length", str(total))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            self.wfile.write(data)

        def _read_json(self):
            try:
                ln = int(self.headers.get("Content-Length", "0") or "0")
            except ValueError:
                self.close_connection = True
                raise ValueError("invalid Content-Length")
            if ln < 0:
                self.close_connection = True
                raise ValueError("invalid Content-Length")
            if ln > MAX_BODY:
                if ln > MAX_DRAIN:
                    self.close_connection = True
                else:
                    left = ln
                    while left > 0:
                        chunk = self.rfile.read(min(left, 65536))
                        if not chunk:
                            self.close_connection = True
                            break
                        left -= len(chunk)
                raise BodyTooLarge()
            if ln == 0:
                return {}
            raw = self.rfile.read(ln)
            try:
                body = json.loads(raw.decode("utf-8"))
            except UnicodeDecodeError as exc:
                raise ValueError(f"invalid body: {exc}")
            if not isinstance(body, dict):
                raise ValueError("body must be a JSON object")
            return body

        def do_GET(self):
            path = urlparse(self.path).path
            try:
                if path == "/":
                    self._send_file(INDEX_HTML, "text/html; charset=utf-8")
                    return
                if path == "/api/pieces":
                    self._send_json(HTTPStatus.OK, {"pieces": store.list_pieces()})
                    return
                if path == "/api/marks.json":
                    self._send_json(HTTPStatus.OK, store.all_marks_export(), download_name="marks.json")
                    return
                parts = [p for p in path.split("/") if p]
                if len(parts) == 3 and parts[:2] == ["api", "piece"]:
                    self._send_json(HTTPStatus.OK, store.get_piece(parts[2]))
                    return
                if len(parts) == 4 and parts[:2] == ["api", "piece"]:
                    pid, res = parts[2], parts[3]
                    if res == "audio.mp3":
                        p = store.file_path(pid, "piece.mp3", log_export=False)
                        self._send_file(p, "audio/mpeg", support_range=True)
                        return
                    if res == "piece.mid":
                        p = store.file_path(pid, "piece.mid", log_export=True)
                        self._send_file(p, "audio/midi", download_name=f"{pid}.mid")
                        return
                    if res == "piece.musicxml":
                        p = store.file_path(pid, "piece.musicxml", log_export=True)
                        self._send_file(
                            p, "application/vnd.recordare.musicxml+xml; charset=utf-8",
                            download_name=f"{pid}.musicxml",
                        )
                        return
                    if res == "marks.json":
                        self._send_json(HTTPStatus.OK, store.marks_export(pid), download_name=f"{pid}.marks.json")
                        return
                    if res == "piece.tab.txt":
                        p = store.file_path(pid, "piece.tab.txt", log_export=True)
                        self._send_file(p, "text/plain; charset=utf-8", download_name=f"{pid}.tab.txt")
                        return
                self._send_json(HTTPStatus.NOT_FOUND, {"error": "not found"})
            except (KeyError, FileNotFoundError) as exc:
                self._send_json(HTTPStatus.NOT_FOUND, {"error": str(exc)})
            except Exception as exc:
                self._send_json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": str(exc)})

        def do_POST(self):
            path = urlparse(self.path).path
            try:
                body = self._read_json()
                if path == "/api/new":
                    seed = body.get("seed")
                    if seed is not None and (
                        isinstance(seed, bool) or not isinstance(seed, int) or not 0 <= seed < MAX_SEED
                    ):
                        raise ValueError("seed must be a non-negative integer or null")
                    self._send_json(HTTPStatus.OK, store.create_piece(seed))
                    return
                parts = [p for p in path.split("/") if p]
                if len(parts) == 4 and parts[:2] == ["api", "piece"]:
                    pid, action = parts[2], parts[3]
                    if action == "reroll":
                        layer, section = _text_field(body, "layer"), _text_field(body, "section")
                        self._send_json(HTTPStatus.OK, store.reroll(pid, layer, section))
                        return
                    if action == "lock":
                        layer, section = _text_field(body, "layer"), _text_field(body, "section")
                        if "locked" not in body:
                            raise ValueError("locked is required")
                        self._send_json(HTTPStatus.OK, store.set_lock(pid, layer, section, body["locked"]))
                        return
                    if action == "option":
                        name = _text_field(body, "name")
                        if "value" not in body:
                            raise ValueError("value is required")
                        self._send_json(HTTPStatus.OK, store.set_option(pid, name, body["value"]))
                        return
                    if action == "mark_add":
                        if "t" not in body:
                            raise ValueError("t is required")
                        self._send_json(
                            HTTPStatus.OK,
                            store.add_mark(
                                pid, body["t"], body.get("guitar", "both"), body.get("text", ""),
                                body.get("id"), body.get("rev"),
                            ),
                        )
                        return
                    if action == "mark_edit":
                        self._send_json(
                            HTTPStatus.OK,
                            store.edit_mark(pid, _text_field(body, "id"), body.get("guitar"), body.get("text")),
                        )
                        return
                    if action == "mark_delete":
                        self._send_json(HTTPStatus.OK, store.delete_mark(pid, _text_field(body, "id")))
                        return
                    if action == "undo":
                        self._send_json(HTTPStatus.OK, store.undo(pid))
                        return
                    if action == "redo":
                        self._send_json(HTTPStatus.OK, store.redo(pid))
                        return
                self._send_json(HTTPStatus.NOT_FOUND, {"error": "not found"})
            except BodyTooLarge:
                self._send_json(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, {"error": "request body too large"})
            except KeyError as exc:
                self._send_json(HTTPStatus.NOT_FOUND, {"error": str(exc)})
            except (ValueError, TypeError, json.JSONDecodeError) as exc:
                self._send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            except Exception as exc:
                self._send_json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": str(exc)})

    return StudioHandler


class DirLock:
    def __init__(self, path):
        self._file = open(path, "a+b")
        try:
            if msvcrt:
                self._file.seek(0)
                msvcrt.locking(self._file.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                fcntl.flock(self._file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            self._file.close()
            raise RuntimeError("Каталог пьес уже занят другим сервером Studio; запустите сервер с другим --out.") from None

    def release(self):
        self._file.close()


class StudioServer(ThreadingHTTPServer):
    dir_lock = None

    def server_close(self):
        super().server_close()
        if self.dir_lock:
            self.dir_lock.release()
            self.dir_lock = None


def create_server(out_dir, host="127.0.0.1", port=8765, render_fn=None):
    store = StudioStore(out_dir, render_fn=render_fn or default_render)
    lock = DirLock(store.out_dir / ".studio.lock")
    try:
        srv = StudioServer((host, port), _make_handler(store))
    except BaseException:
        lock.release()
        raise
    srv.dir_lock = lock
    return srv


def main(argv=None):
    ap = argparse.ArgumentParser(description="Studio curation web server for composer-lab")
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--out", default="out_studio")
    ap.add_argument("--soundfont", default=None)
    ap.add_argument("--fake-render", action="store_true")
    args = ap.parse_args(argv)

    if args.fake_render:
        rfn = fake_render
    elif args.soundfont:
        rfn = lambda mid, mp3: default_render(mid, mp3, soundfont=args.soundfont)
    else:
        rfn = default_render

    try:
        srv = create_server(args.out, host="127.0.0.1", port=args.port, render_fn=rfn)
    except RuntimeError as exc:
        print(exc, file=sys.stderr)
        return 2
    print(f"Studio listening on http://127.0.0.1:{srv.server_address[1]} (out={args.out})", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
