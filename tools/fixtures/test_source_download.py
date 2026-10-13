import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest
from email.message import Message
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest import mock
from urllib.request import Request

_spec = importlib.util.spec_from_file_location('regenerate', Path(__file__).with_name('regenerate.py'))
regenerate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(regenerate)


class SourceServer:
    def __init__(self, respond):
        self.workers = []
        workers = self.workers

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                workers.append(threading.current_thread())
                try:
                    respond(self)
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def log_message(self, *args):
                pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.proxy = mock.patch.dict(os.environ, {
            'NO_PROXY': '127.0.0.1,localhost', 'no_proxy': '127.0.0.1,localhost',
        })

    def __enter__(self):
        self.proxy.start()
        self.thread.start()
        return f'http://127.0.0.1:{self.server.server_port}'

    def __exit__(self, *args):
        try:
            self.server.shutdown()
            self.server.server_close()
            self.thread.join()
            for worker in self.workers:
                worker.join(timeout=5)
                if worker.is_alive():
                    raise AssertionError('test HTTP worker was not collected')
        finally:
            self.proxy.stop()


def body_response(data, content_length=True):
    def respond(handler):
        handler.send_response(200)
        if content_length:
            handler.send_header('Content-Length', str(len(data)))
        handler.end_headers()
        handler.wfile.write(data)
    return respond


class SourceDownloadTests(unittest.TestCase):
    def test_remote_sources_reject_non_http_schemes_before_opening(self):
        for location in ['ftp://example.test/font.ttf', 'data:,font',
                         'file:///font.ttf']:
            with self.subTest(location=location), \
                    mock.patch.object(regenerate, 'build_opener') as opener:
                with self.assertRaisesRegex(ValueError, 'HTTP'):
                    regenerate._copy_source(location, True, 8, io.BytesIO())
                opener.assert_not_called()

    def test_redirects_reject_ftp_before_opening_the_control_connection(self):
        for code in [301, 302, 303, 307, 308]:
            with self.subTest(code=code):
                handler = regenerate._RedirectWithoutBody()
                handler.parent = mock.Mock()
                request = Request('https://example.test/start')
                request.timeout = 60
                headers = Message()
                headers['Location'] = 'ftp://example.test/font.ttf'
                response = mock.Mock()
                with self.assertRaisesRegex(ValueError, 'HTTP'):
                    getattr(handler, f'http_error_{code}')(
                        request, response, code, 'redirect', headers)
                handler.parent.open.assert_not_called()
                response.close.assert_called()
                response.read.assert_not_called()

    def test_copy_reads_only_bounded_chunks_and_never_publishes_overflow(self):
        class Input(io.BytesIO):
            def read(self, count=-1):
                self_requests.append(count)
                if count < 0:
                    raise AssertionError('unbounded source read')
                return super().read(count)

        self_requests = []
        stream = Input(b'a' * 80)
        output = io.BytesIO()
        with self.assertRaises(regenerate._SourceTooLarge):
            regenerate._copy_bounded(stream, output, 8)
        self.assertEqual(self_requests, [9])
        self.assertEqual(stream.tell(), 9)
        self.assertEqual(output.getvalue(), b'')
        self_requests.clear()
        regenerate._copy_bounded(Input(b'12345678'), output, 8)
        self.assertEqual(output.getvalue(), b'12345678')
        self.assertEqual(self_requests, [9, 1])

    def test_local_source_one_over_is_rejected_before_buffering_or_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'source.ttf'
            path.write_bytes(b'123456789')
            with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8):
                with self.assertRaisesRegex(ValueError, 'source.*bytes'):
                    regenerate._read_source(path)

    def test_remote_one_over_without_content_length_is_rejected(self):
        with SourceServer(body_response(b'123456789', content_length=False)) as url:
            with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8):
                with self.assertRaisesRegex(ValueError, 'source.*bytes'):
                    regenerate._read_source(url, remote=True)

    def test_known_length_and_chunked_one_over_are_rejected(self):
        def chunked(handler):
            handler.send_response(200)
            handler.send_header('Transfer-Encoding', 'chunked')
            handler.end_headers()
            handler.wfile.write(b'4\r\n1234\r\n5\r\n56789\r\n0\r\n\r\n')

        for respond in [body_response(b'123456789'), chunked]:
            with self.subTest(respond=respond.__name__), SourceServer(respond) as url:
                with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8):
                    with self.assertRaisesRegex(ValueError, 'source.*bytes'):
                        regenerate._read_source(url, remote=True)

    def test_chunked_exact_cap_and_missing_length_preserve_bytes(self):
        def chunked(handler):
            handler.send_response(200)
            handler.send_header('Transfer-Encoding', 'chunked')
            handler.end_headers()
            handler.wfile.write(b'4\r\n1234\r\n4\r\n5678\r\n0\r\nX-Finished: yes\r\n\r\n')

        for respond in [body_response(b'12345678', False), chunked]:
            with self.subTest(respond=respond.__name__), SourceServer(respond) as url:
                with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8):
                    self.assertEqual(regenerate._read_source(url, remote=True), b'12345678')

    def test_all_redirect_handlers_close_without_reading_the_body(self):
        for code in [301, 302, 303, 307, 308]:
            with self.subTest(code=code):
                handler = regenerate._RedirectWithoutBody()
                handler.parent = mock.Mock()
                request = Request('http://example.test/start')
                request.timeout = 60
                headers = Message()
                headers['Location'] = '/target'
                response = mock.Mock()
                response.read.side_effect = AssertionError('redirect body was drained')
                result = getattr(handler, f'http_error_{code}')(
                    request, response, code, 'redirect', headers)
                response.close.assert_called()
                response.read.assert_not_called()
                self.assertIs(result, handler.parent.open.return_value)
                forwarded = handler.parent.open.call_args.args[0]
                self.assertEqual(forwarded.full_url, 'http://example.test/target')

    def test_live_redirects_keep_source_bytes_without_waiting_for_the_body(self):
        for code in [301, 302, 303, 307, 308]:
            def respond(handler):
                if handler.path == '/target':
                    body_response(b'12345678')(handler)
                else:
                    handler.send_response(code)
                    handler.send_header('Location', '/target')
                    handler.send_header('Content-Length', '99999999')
                    handler.end_headers()
                    # No redirect body arrives. Only the final source body is
                    # relevant; draining here would exhaust the outer deadline.
                    time.sleep(1.5)

            with self.subTest(code=code), SourceServer(respond) as url:
                with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8), \
                        mock.patch.object(regenerate, 'SOURCE_TRANSFER_SECONDS', 1):
                    self.assertEqual(regenerate._read_source(url, remote=True), b'12345678')

    def test_redirect_loop_and_truncated_body_fail_and_reap_workers(self):
        def loop(handler):
            handler.send_response(302)
            handler.send_header('Location', '/loop')
            handler.send_header('Content-Length', '0')
            handler.end_headers()

        def truncated(handler):
            handler.send_response(200)
            handler.send_header('Transfer-Encoding', 'chunked')
            handler.end_headers()
            handler.wfile.write(b'8\r\n123')

        def truncated_length(handler):
            handler.send_response(200)
            handler.send_header('Content-Length', '8')
            handler.end_headers()
            handler.wfile.write(b'123')

        original = regenerate.subprocess.Popen
        for respond in [loop, truncated, truncated_length]:
            children = []

            def start(*args, **kwargs):
                child = original(*args, **kwargs)
                children.append(child)
                return child

            with self.subTest(respond=respond.__name__), SourceServer(respond) as url:
                with mock.patch.object(regenerate.subprocess, 'Popen', side_effect=start):
                    with self.assertRaisesRegex(ValueError, 'source read failed'):
                        regenerate._read_source(url, remote=True)
            self.assertEqual(len(children), 1)
            self.assertIsNotNone(children[0].poll())
            self.assertTrue(children[0].stdout.closed)

    def test_total_deadline_covers_slow_status_headers_body_chunk_metadata_and_trailers(self):
        original = regenerate.subprocess.Popen
        for phase in ['status', 'headers', 'body', 'chunk', 'trailer']:
            entered = threading.Event()
            children = []

            def start(*args, **kwargs):
                child = original(*args, **kwargs)
                children.append(child)
                return child

            def respond(handler):
                entered.set()
                if phase == 'status':
                    prefix, drip = b'', b'HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n12345678'
                elif phase == 'headers':
                    prefix = b'HTTP/1.1 200 OK\r\nX-Slow: '
                    drip = b'value\r\nContent-Length: 8\r\n\r\n12345678'
                elif phase == 'body':
                    prefix = b'HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n'
                    drip = b'12345678'
                elif phase == 'chunk':
                    prefix = b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n'
                    drip = b'8;slow=value\r\n12345678\r\n0\r\n\r\n'
                else:
                    prefix = b'HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n12345678\r\n0\r\n'
                    drip = b'X-Slow: value\r\n\r\n'
                handler.wfile.write(prefix)
                for byte in drip:
                    handler.wfile.write(bytes([byte]))
                    time.sleep(0.15)

            with self.subTest(phase=phase), SourceServer(respond) as url:
                with mock.patch.object(regenerate, 'SOURCE_TRANSFER_SECONDS', 0.6), \
                        mock.patch.object(regenerate.subprocess, 'Popen', side_effect=start):
                    with self.assertRaisesRegex(ValueError, 'source.*deadline'):
                        regenerate._read_source(url, remote=True)
                self.assertTrue(entered.is_set(), 'worker must reach the tested HTTP phase')
                self.assertEqual(len(children), 1)
                self.assertIsNotNone(children[0].poll())
                self.assertTrue(children[0].stdout.closed)

    def test_pre_response_wait_is_killed_and_interruptions_reap_workers(self):
        original = regenerate.subprocess.Popen
        for interruption in [None, KeyboardInterrupt, RuntimeError]:
            children = []

            def start(*args, **kwargs):
                child = original(*args, **kwargs)
                children.append(child)
                if interruption is not None:
                    child.communicate = mock.Mock(side_effect=interruption)
                return child

            with self.subTest(interruption=interruption), \
                    mock.patch.object(regenerate, '_SOURCE_WORKER', 'import time; time.sleep(30)'), \
                    mock.patch.object(regenerate, 'SOURCE_TRANSFER_SECONDS', 0.1), \
                    mock.patch.object(regenerate.subprocess, 'Popen', side_effect=start):
                exception = ValueError if interruption is None else interruption
                with self.assertRaises(exception):
                    regenerate._read_source('unused', remote=True)
            self.assertEqual(len(children), 1)
            self.assertIsNotNone(children[0].poll())
            self.assertTrue(children[0].stdout.closed)

    def test_offline_check_never_starts_source_worker(self):
        root = Path(__file__).resolve().parents[2] / 'dev/fixtures'
        with mock.patch.object(regenerate.subprocess, 'Popen', side_effect=AssertionError('offline check started a worker')):
            regenerate.check_assets(root)


    def test_exact_cap_local_and_remote_keep_original_bytes_and_checksum(self):
        data = b'12345678'
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'source.ttf'
            path.write_bytes(data)
            with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8):
                self.assertEqual(regenerate._read_source(path), data)
                with SourceServer(body_response(data)) as url:
                    received = regenerate._read_source(url, remote=True)
                    self.assertEqual(received, data)
                    regenerate.validate_source(received, regenerate.digest(data))

    def test_later_oversized_source_leaves_update_assets_and_manifest_untouched(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'assets').mkdir()
            (root / 'sources').mkdir()
            entries = []
            for index, data in enumerate([b'12345678', b'123456789']):
                filename = f'{index}.ttf'
                (root / 'sources' / filename).write_bytes(data)
                (root / 'assets' / filename).write_bytes(b'original asset')
                entries.append(dict(id=str(index), path=f'assets/{filename}',
                                    source_file=filename, source_sha256=regenerate.digest(data)))
            (root / 'assets/cases.json').write_text('[]', encoding='utf-8')
            (root / 'assets/manifest.json').write_text(json.dumps({
                'format_version': 1, 'fonts': entries,
            }), encoding='utf-8')
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8), \
                    mock.patch.object(regenerate, 'subset_font', return_value=b'new font') as subset:
                with self.assertRaisesRegex(ValueError, 'source.*bytes'):
                    regenerate.rebuild(root, root / 'sources', update=True)
            self.assertEqual(subset.call_count, 1)
            self.assertEqual(before, {p: p.read_bytes() for p in root.rglob('*') if p.is_file()})

    def test_later_remote_failures_leave_both_rebuild_modes_unmodified(self):
        for update in [False, True]:
            for failure in ['size', 'checksum', 'deadline']:
                def respond(handler):
                    if handler.path == '/first':
                        body_response(b'12345678')(handler)
                    elif failure == 'size':
                        body_response(b'123456789', False)(handler)
                    elif failure == 'checksum':
                        body_response(b'abcdefgh')(handler)
                    else:
                        handler.send_response(200)
                        handler.send_header('Content-Length', '8')
                        handler.end_headers()
                        time.sleep(1.5)
                        handler.wfile.write(b'12345678')

                with self.subTest(update=update, failure=failure), \
                        tempfile.TemporaryDirectory() as directory, SourceServer(respond) as url:
                    root = Path(directory)
                    (root / 'assets').mkdir()
                    entries = []
                    for index, suffix in enumerate(['first', 'second']):
                        (root / f'assets/{index}.ttf').write_bytes(b'original asset')
                        entries.append(dict(id=str(index), path=f'assets/{index}.ttf',
                                            source_url=f'{url}/{suffix}',
                                            source_sha256=regenerate.digest(b'12345678'),
                                            sha256=regenerate.digest(b'new font')))
                    (root / 'assets/cases.json').write_text('[]', encoding='utf-8')
                    (root / 'assets/manifest.json').write_text(json.dumps({
                        'format_version': 1, 'fonts': entries,
                    }), encoding='utf-8')
                    before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
                    with mock.patch.object(regenerate, 'SOURCE_MAX_BYTES', 8), \
                            mock.patch.object(regenerate, 'SOURCE_TRANSFER_SECONDS', 0.6), \
                            mock.patch.object(regenerate, 'subset_font', return_value=b'new font') as subset:
                        expected = {'size': 'source.*bytes', 'checksum': 'source checksum',
                                    'deadline': 'source.*deadline'}[failure]
                        with self.assertRaisesRegex(ValueError, expected):
                            regenerate.rebuild(root, update=update)
                    self.assertEqual(subset.call_count, 1)
                    self.assertEqual(before, {p: p.read_bytes() for p in root.rglob('*') if p.is_file()})


if __name__ == '__main__':
    unittest.main()
