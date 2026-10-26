#!/usr/bin/env python3
"""Run the beginner documentation's programs against a real Kiroha executable."""
import argparse
from decimal import Decimal
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[2]
PURE = ('index.md', 'getting-started.md', 'learn/functions.md', 'learn/data.md', 'guides/troubleshooting.md')
EXPECTED = {('getting-started.md', 0): '"Hello, Kiroha!"', ('getting-started.md', 1): '0.3'}


def fences(path, language):
    return re.findall(r'^```' + language + r'[^\n]*\n(.*?)^```\s*$', path.read_text(), re.M | re.S)


def untag(value):
    kind = value['t']
    if kind == 'number':
        return Decimal(value['v'])
    if kind == 'array':
        return [untag(v) for v in value['v']]
    if kind == 'record':
        return {k: untag(v) for k, v in value['v']}
    if kind == 'null':
        return None
    if kind in ('bool', 'string'):
        return value['v']
    raise AssertionError(f'Unexpected tutorial result kind: {kind}')


def run(binary, source, expected, host=None):
    args = [binary, 'run', '-', '--json']
    if host:
        args += ['--host', str(host)]
    result = subprocess.run(args, input=source, text=True, capture_output=True, timeout=20)
    assert result.returncode == 0, result.stdout + result.stderr
    payload = json.loads(result.stdout)
    want = json.loads(expected, parse_int=Decimal, parse_float=Decimal)
    assert payload['ok'] and untag(payload['result']) == want, (source, payload, want)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kiroha', default='kiroha')
    args = parser.parse_args()
    count = 0
    for name in PURE:
        snippets = fences(ROOT / 'docs' / name, 'kiroha')
        assert snippets, f'No beginner programs found in {name}'
        for index, source in enumerate(snippets):
            expected = EXPECTED.get((name, index))
            if expected is None:
                match = re.search(r'//\s*(.+)\s*\Z', source.rstrip())
                assert match, f'Add an expected JSON result to {name}, block {index}'
                expected = match[1]
            run(args.kiroha, source, expected)
            count += 1
    page = ROOT / 'docs/guides/http.md'
    items, host = [json.loads(s) for s in fences(page, 'json')]
    class QuietHandler(SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass
    with tempfile.TemporaryDirectory(prefix='kiroha-doc-http.') as folder:
        directory = Path(folder)
        (directory / 'items.json').write_text(json.dumps(items))
        host_path = directory / 'local.host.json'
        host_path.write_text(json.dumps(host))
        server = ThreadingHTTPServer(('127.0.0.1', 8765), partial(QuietHandler, directory=folder))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            run(args.kiroha, fences(page, 'kiroha')[0], '["Tea","Cake"]', host_path)
            count += 1
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
    print(f'{count} beginner programs passed, including the documented HTTP request.')


if __name__ == '__main__':
    main()
