#!/usr/bin/env python3
"""Sync, verify and package the Kiroha skills-only plugin (Python 3.11+)."""
import argparse
from decimal import Decimal
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import threading
import time
import tomllib
from urllib.parse import urlsplit
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLUGIN = ROOT / 'plugins/kiroha'
SKILL = PLUGIN / 'skills/write-kiroha'
EXAMPLES = SKILL / 'assets/examples'
SITE = 'https://kiroha-lang.github.io/kiroha/'
SCHEMA = 'https://agent-plugins.org/schemas/1.0.0/plugin.schema.json'


def map_markdown_links(text, transform):
    """Transform prose links without mistaking fenced or inline code for links."""
    fence = None
    output = []
    inline = re.compile(r'(?<!`)(`+)(?!`)(.*?)(?<!`)\1(?!`)')
    for line in text.splitlines(keepends=True):
        marker = re.match(r'^ {0,3}(`{3,}|~{3,})', line)
        if fence:
            output.append(line)
            close = re.match(r'^ {0,3}(`{3,}|~{3,})[ \t]*(?:\r?\n)?$', line)
            if close and close[1][0] == fence[0] and len(close[1]) >= len(fence):
                fence = None
            continue
        if marker:
            fence = marker[1]
            output.append(line)
            continue
        pos = 0
        links = re.compile(r'\[([^\]]+)\]\(([^)]+)\)')
        while pos < len(line):
            code = inline.search(line, pos)
            link = links.search(line, pos)
            if link and (not code or link.start() < code.start()):
                output.append(line[pos:link.start()])
                output.append(transform(link))
                pos = link.end()
            elif code:
                output.append(line[pos:code.end()])
                pos = code.end()
            else:
                output.append(line[pos:])
                break
    return ''.join(output)


def generated():
    spec = (ROOT / 'spec.md').read_text()
    start = '<!-- KIROHA_GENERATION_CARD_BEGIN -->\n```text\n'
    end = '```\n<!-- KIROHA_GENERATION_CARD_END -->'
    assert spec.count(start) == spec.count(end) == 1
    card = spec.split(start)[1].split(end)[0]
    result = {SKILL / 'references/generation-card.md': card}
    codex = read_json(PLUGIN / 'plugin.json')
    language = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    language_series = '.'.join(language.split('.')[:2])
    claude = {
        'name': codex['name'], 'version': codex['version'],
        'description': f'Write, review and migrate Kiroha {language_series} programs with runnable examples.',
        'author': codex['author'], 'homepage': SITE + 'claude-plugin/',
        'repository': codex['repository'], 'license': codex['license'],
        'keywords': ['kiroha', 'programming-language', 'code-generation'],
        'icon': './.claude-plugin/icon.png',
    }
    result[PLUGIN / '.claude-plugin' / 'plugin.json'] = json.dumps(claude, ensure_ascii=False, indent=2) + '\n'
    for source in ('reference/builtins.md', 'host-config.md', 'guides/databases.md', 'types.md', 'migration-0.6.md', 'migration-0.8.md'):
        text = (ROOT / 'docs' / source).read_text()
        def absolute(m):
            label, link = m.groups()
            if link.startswith(('#', 'https://', 'http://')):
                return m[0]
            target = urlsplit(link)
            local = ((ROOT / 'docs' / source).parent / target.path).resolve()
            if local == ROOT / 'spec.md':
                url = SITE + 'spec/'
            elif local.is_relative_to(ROOT / 'docs'):
                url = SITE + local.relative_to(ROOT / 'docs').with_suffix('').as_posix() + '/'
            else:
                url = 'https://github.com/kiroha-lang/kiroha/blob/main/' + local.relative_to(ROOT).as_posix()
            if target.fragment:
                url += '#' + target.fragment
            return f'[{label}]({url})'
        text = map_markdown_links(text, absolute)
        result[SKILL / 'references' / Path(source).name] = (
            f'<!-- Generated from docs/{source}; run python3 scripts/plugin.py sync. -->\n\n' + text)
    return result


def sync(check=False):
    for path, expected in generated().items():
        if check:
            assert path.is_file() and path.read_text() == expected, f'{path.relative_to(ROOT)} is stale: run python3 scripts/plugin.py sync'
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(expected)
    icon = PLUGIN / '.claude-plugin' / 'icon.png'
    source_icon = ROOT / 'assets/branding/kiroha-icon.png'
    if check:
        assert icon.is_file() and icon.read_bytes() == source_icon.read_bytes(), 'Claude plugin icon is stale'
    else:
        icon.parent.mkdir(parents=True, exist_ok=True)
        icon.write_bytes(source_icon.read_bytes())


def read_json(path):
    return json.loads(path.read_text())


def metadata():
    import yaml
    sync(check=True)
    manifest = read_json(PLUGIN / 'plugin.json')
    assert manifest['$schema'] == SCHEMA
    assert manifest['name'] == PLUGIN.name
    assert re.fullmatch(r'\d+\.\d+\.\d+', manifest['version'])
    interface = manifest['extensions']['com.openai']['interface']
    assert 1 <= len(interface['shortDescription']) <= 30
    assert interface['category'] == 'Developer Tools'
    assert isinstance(interface['capabilities'], list)
    marketplace = read_json(ROOT / '.agents/plugins/marketplace.json')
    entry, = marketplace['plugins']
    assert entry['name'] == manifest['name']
    assert (ROOT / entry['source']['path']).resolve() == PLUGIN
    assert entry['policy']['installation'] == 'AVAILABLE'
    assert entry['category'] == interface['category']
    claude_marketplace = read_json(ROOT / '.claude-plugin/marketplace.json')
    claude_entry, = claude_marketplace['plugins']
    assert claude_marketplace['name'] == marketplace['name']
    assert claude_entry['name'] == manifest['name']
    assert (ROOT / claude_entry['source']).resolve() == PLUGIN
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    assert (SKILL / 'references/generation-card.md').read_text().startswith(f'Kiroha {version} / Web1\n')
    for text_file in (SKILL / 'SKILL.md', SKILL / 'references/runtime.md'):
        declared = re.search(r'Kiroha (\d+\.\d+\.\d+)', text_file.read_text())
        assert declared and declared[1] == version, f'{text_file}: update the target language version'
    assert {p.name for p in PLUGIN.iterdir()} == {'plugin.json', 'LICENSE', 'skills', '.claude-plugin'}
    skills = list((PLUGIN / 'skills').glob('*/SKILL.md'))
    assert len(skills) == 1
    for path in skills:
        text = path.read_text()
        assert text.startswith('---\n')
        front = yaml.safe_load(text.split('---', 2)[1])
        assert front['name'] == path.parent.name
        assert isinstance(front['description'], str) and front['description'].strip()
        ui = yaml.safe_load((path.parent / 'agents/openai.yaml').read_text())
        assert ui['interface']['display_name'].strip()
        assert 25 <= len(ui['interface']['short_description']) <= 64
        assert '$' + front['name'] in ui['interface']['default_prompt']
        assert ui.get('policy', {}).get('allow_implicit_invocation', True) is True
    for path in PLUGIN.rglob('*'):
        assert not path.is_symlink(), f'Symlink in plugin: {path}'
        if path.is_file() and path.suffix == '.json':
            read_json(path)
        if path.is_file() and path.suffix == '.md':
            text = path.read_text()
            # The generation card is plain source text, not Markdown prose.
            if path == SKILL / 'references/generation-card.md':
                continue
            def verify_link(match):
                link = match[2]
                if link.startswith(('https://', 'http://', '#')):
                    return match[0]
                target = (path.parent / link.split('#', 1)[0]).resolve()
                assert target.is_relative_to(PLUGIN), (path, link)
                assert target.exists(), (path, link)
                return match[0]
            map_markdown_links(text, verify_link)
    return manifest


def represented(v):
    kind = v['t']
    if kind == 'number':
        return Decimal(v['v'])
    if kind == 'record':
        return {k: represented(value) for k, value in v['v']}
    if kind == 'array':
        return [represented(value) for value in v['v']]
    if kind == 'null':
        return None
    assert kind in ('string', 'bool'), kind
    return v['v']


def cli(binary, command, source, host=None):
    args = [str(binary), command, '-', '--json']
    if host:
        args += ['--host', str(host)]
    result = subprocess.run(args, input=source, capture_output=True, text=True, timeout=15)
    assert result.returncode == 0, result.stdout + result.stderr
    payload = json.loads(result.stdout)
    assert payload['ok'], payload
    return payload


def expected(source):
    match = re.search(r'^// => (.+)$', source, re.M)
    assert match, 'Example needs an expected result'
    return json.loads(match[1], parse_float=Decimal, parse_int=Decimal)


def verify_programs(binary):
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    info = json.loads(subprocess.check_output([str(binary), '--version'], text=True))
    assert info['language'] == version, info
    card = subprocess.check_output([str(binary), 'spec', '--compact'], text=True)
    assert card == (SKILL / 'references/generation-card.md').read_text()
    examples = sorted(EXAMPLES.glob('*.kiroha'))
    for path in examples:
        cli(binary, 'check', path.read_text())
    for name in ('summarize', 'callbacks', 'validation'):
        source = (EXAMPLES / f'{name}.kiroha').read_text()
        assert represented(cli(binary, 'run', source)['result']) == expected(source)
    with tempfile.TemporaryDirectory(prefix='kiroha-plugin.') as folder:
        tmp = Path(folder)
        # Real HTTP fixture, including paginated responses and bounded parallel calls.
        class Fixture(BaseHTTPRequestHandler):
            def do_GET(self):
                base = f'http://127.0.0.1:{self.server.server_port}'
                routes = {
                    '/items': [{'id': 1, 'price': 1}, {'id': 2, 'price': 3}, {'id': 3, 'price': 4}],
                    '/page/1': {'items': [{'id': 1}], 'next': base + '/page/2'},
                    '/page/2': {'items': [{'id': 2}], 'next': None},
                    '/detail/1': {'id': 1, 'name': 'Tea'}, '/detail/2': {'id': 2, 'name': 'Cake'},
                }
                if self.path not in routes:
                    self.send_error(404)
                    return
                body = json.dumps(routes[self.path]).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            def log_message(self, *_):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base = f'http://127.0.0.1:{server.server_port}'
        host = read_json(EXAMPLES / 'client.host.json')
        host['capabilities']['http'] = [base]
        host_path = tmp / 'client.host.json'
        host_path.write_text(json.dumps(host))
        try:
            for name in ('fetch', 'pagination'):
                source = (EXAMPLES / f'{name}.kiroha').read_text()
                actual_source = source.replace('http://127.0.0.1:8765', base)
                assert represented(cli(binary, 'run', actual_source, host_path)['result']) == expected(source)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
        # Actual filesystem IO stays inside the task-owned temporary directory.
        (tmp / 'input.json').write_text('[{"price":2.5,"quantity":2},{"price":4.2,"quantity":1}]')
        host_path.write_text(json.dumps({'format': 'kiroha-host/1', 'filesystem_root': str(tmp),
            'capabilities': {'read': [str(tmp)], 'write': [str(tmp)]}}))
        source = (EXAMPLES / 'files.kiroha').read_text()
        assert represented(cli(binary, 'run', source, host_path)['result']) == expected(source)
        assert json.loads((tmp / 'summary.json').read_text(), parse_float=Decimal) == expected(source)
        verify_service(binary, tmp)
    print(f'{len(examples)} examples statically checked; pure, HTTP/pagination, files and live service passed. DB example checked only.')


def verify_service(binary, tmp):
    import socket
    import urllib.error
    import urllib.request
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    source = (EXAMPLES / 'service.kiroha').read_text().replace('port:8080', f'port:{port}')
    program = tmp / 'service.kiroha'
    program.write_text(source)
    host = read_json(EXAMPLES / 'service.host.json')
    host['capabilities']['listen'][0]['port'] = port
    config = tmp / 'service.host.json'
    config.write_text(json.dumps(host))
    proc = subprocess.Popen([str(binary), 'run', str(program), '--json', '--host', str(config)],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    base = f'http://127.0.0.1:{port}'
    try:
        ready = False
        until = time.monotonic() + 8
        while time.monotonic() < until:
            if proc.poll() is not None:
                raise AssertionError(proc.communicate())
            try:
                with urllib.request.urlopen(base + '/health', timeout=0.2) as response:
                    assert json.load(response) == {'ok': True}
                    ready = True
                    break
            except (OSError, urllib.error.URLError):
                time.sleep(0.05)
        assert ready, 'Service did not become ready'
        request = urllib.request.Request(base + '/total', data=b'[{"price":2.5,"quantity":2},{"price":4.2,"quantity":1}]',
            headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=5) as response:
            assert json.loads(response.read(), parse_float=Decimal) == {'total': Decimal('9.2')}
        bad = urllib.request.Request(base + '/total', data=b'[{"price":"wrong","quantity":1}]',
            headers={'Content-Type': 'application/json'})
        try:
            urllib.request.urlopen(bad, timeout=5)
            raise AssertionError('Invalid payload was accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 400
    finally:
        if proc.poll() is None:
            proc.send_signal(signal.SIGINT)
        try:
            proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.communicate()


def package(output):
    manifest = metadata()
    output.parent.mkdir(parents=True, exist_ok=True)
    assert not output.resolve().is_relative_to(PLUGIN), 'Write the archive outside the plugin directory'
    files = sorted(p for p in PLUGIN.rglob('*') if p.is_file())
    with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for path in files:
            info = zipfile.ZipInfo('kiroha/' + path.relative_to(PLUGIN).as_posix())
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, path.read_bytes())
    with zipfile.ZipFile(output) as archive:
        assert archive.testzip() is None
        assert set(archive.namelist()) == {'kiroha/' + p.relative_to(PLUGIN).as_posix() for p in files}
        for path in files:
            assert archive.read('kiroha/' + path.relative_to(PLUGIN).as_posix()) == path.read_bytes()
    print(f'{output}: {manifest["name"]} {manifest["version"]}, {len(files)} files, inspected')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    command = commands.add_parser('sync', help='refresh references from the canonical specification/docs')
    command.add_argument('--check', action='store_true')
    command = commands.add_parser('check', help='verify package and run examples with Kiroha')
    command.add_argument('--kiroha', type=Path, required=True)
    command = commands.add_parser('package', help='build and inspect a reproducible ZIP')
    command.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.command == 'sync':
        sync(args.check)
        print('Plugin references match canonical sources.' if args.check else 'Plugin references synchronized.')
    elif args.command == 'check':
        metadata()
        verify_programs(args.kiroha.resolve())
        print('Plugin metadata, reference links and runtime card match.')
    else:
        version = read_json(PLUGIN / 'plugin.json')['version']
        package(args.output or ROOT / f'target/plugins/kiroha-{version}.zip')


if __name__ == '__main__':
    main()
