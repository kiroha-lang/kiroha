#!/usr/bin/env python3
"""Run the beginner documentation's programs against a real Kiroha executable."""
import argparse
from decimal import Decimal
import json
from pathlib import Path
import re
import subprocess
import tempfile
import socket
import time

ROOT = Path(__file__).resolve().parents[2]
PURE = ('index.md', 'getting-started.md', 'learn/functions.md', 'learn/data.md', 'guides/troubleshooting.md', 'types.md')
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
    migration = fences(ROOT / 'docs/migration-0.8.md', 'kiroha')
    assert len(migration) == 3
    for source, expected in zip(migration[:2], ('3', '1')):
        run(args.kiroha, source, expected)
        count += 1
    sql_check = subprocess.run([args.kiroha, 'check', '-', '--json'],
                               input=migration[2], text=True, capture_output=True, timeout=20)
    assert sql_check.returncode == 0, sql_check.stdout + sql_check.stderr
    page = ROOT / 'docs/guides/http.md'
    items, server_host, client_host = [json.loads(s) for s in fences(page, 'json')]
    server_source, client_source = fences(page, 'kiroha')
    with tempfile.TemporaryDirectory(prefix='kiroha-doc-http.') as folder:
        directory = Path(folder)
        (directory / 'items.json').write_text(json.dumps(items))
        # Use a free local port so the example can run alongside other tests.
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        server_source = server_source.replace('8765', str(port))
        client_source = client_source.replace('8765', str(port))
        server_host['filesystem_root'] = folder
        server_host['capabilities']['read'] = [folder]
        server_host['capabilities']['listen'][0]['port'] = port
        client_host['capabilities']['http'] = [f'http://127.0.0.1:{port}']
        server_path = directory / 'server.kiroha'
        server_path.write_text(server_source)
        server_host_path = directory / 'server.host.json'
        server_host_path.write_text(json.dumps(server_host))
        client_host_path = directory / 'local.host.json'
        client_host_path.write_text(json.dumps(client_host))
        server = subprocess.Popen([args.kiroha, 'run', str(server_path), '--json', '--host', str(server_host_path)],
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            for _ in range(100):
                if server.poll() is not None:
                    raise AssertionError('Kiroha server exited: ' + ''.join(server.communicate()))
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=.1):
                        break
                except OSError:
                    time.sleep(.05)
            else:
                raise AssertionError('Kiroha server did not start')
            run(args.kiroha, client_source, '["Tea","Cake"]', client_host_path)
            count += 2
        finally:
            server.terminate()
            try:
                server.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.communicate()
    print(f'{count} beginner programs passed, including the Kiroha HTTP server and request.')


if __name__ == '__main__':
    main()
