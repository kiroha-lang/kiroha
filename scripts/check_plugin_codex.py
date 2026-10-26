#!/usr/bin/env python3
"""Verify real Codex marketplace installation and skill discovery in temporary state.

Requires a Codex CLI with plugin and app-server support; makes no model calls.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--codex', default='codex')
    parser.add_argument('--source', default=str(ROOT), help='local checkout or Git marketplace source')
    parser.add_argument('--ref', help='Git ref for a remote marketplace test')
    args = parser.parse_args()
    manifest = json.loads((ROOT / 'plugins/kiroha/plugin.json').read_text())
    with tempfile.TemporaryDirectory(prefix='kiroha-codex.') as folder:
        temporary = Path(folder)
        home = temporary / 'codex'
        home.mkdir()
        workspace = temporary / 'workspace'
        workspace.mkdir()
        # Override only the child process's supported config-home setting.
        # The caller's config, installs and shell environment remain untouched.
        env = dict(os.environ, CODEX_HOME=str(home))
        def command(*arguments):
            result = subprocess.run([args.codex, *arguments, '--json'], cwd=workspace,
                env=env, capture_output=True, text=True, timeout=90)
            assert result.returncode == 0, result.stderr + result.stdout
            return json.loads(result.stdout)
        add = ['plugin', 'marketplace', 'add', args.source]
        if args.ref:
            add += ['--ref', args.ref]
        marketplace = command(*add)
        assert marketplace['marketplaceName'] == 'kiroha-lang', marketplace
        listing = command('plugin', 'list', '--marketplace', 'kiroha-lang', '--available')
        available = [p for p in listing['available'] if p['pluginId'] == 'kiroha@kiroha-lang']
        assert len(available) == 1, listing
        installed = command('plugin', 'add', 'kiroha@kiroha-lang')
        assert installed['version'] == manifest['version'], installed
        path = Path(installed['installedPath'])
        for source in (ROOT / 'plugins/kiroha').rglob('*'):
            if source.is_file():
                assert (path / source.relative_to(ROOT / 'plugins/kiroha')).read_bytes() == source.read_bytes(), source
        listing = command('plugin', 'list', '--marketplace', 'kiroha-lang')
        plugin, = [p for p in listing['installed'] if p['pluginId'] == 'kiroha@kiroha-lang']
        assert plugin['installed'] and plugin['enabled'], plugin
        # Use the actual host discovery endpoint without starting an agent turn.
        with (temporary / 'app-server.log').open('w') as log:
            proc = subprocess.Popen([args.codex, 'app-server', '--listen', 'stdio://'],
                cwd=workspace, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=log, text=True, bufsize=1)
            replies = queue.Queue()
            def receive():
                for line in proc.stdout:
                    replies.put(json.loads(line))
            reader = threading.Thread(target=receive, daemon=True)
            reader.start()
            def send(message):
                proc.stdin.write(json.dumps(message) + '\n')
                proc.stdin.flush()
            def request(id, method, params):
                send({'id': id, 'method': method, 'params': params})
                while True:
                    reply = replies.get(timeout=30)
                    if reply.get('id') == id:
                        assert 'error' not in reply, reply
                        return reply['result']
            try:
                request(1, 'initialize', {'clientInfo': {'name': 'kiroha-plugin-check', 'version': '0.1.0'},
                    'capabilities': {'experimentalApi': True}})
                send({'method': 'initialized'})
                response = request(2, 'skills/list', {'cwds': [str(workspace)], 'forceReload': True})
                skills = [s for item in response['data'] for s in item['skills']
                    if s['name'].endswith('write-kiroha')]
                assert len(skills) == 1, response
                skill = skills[0]
                assert skill['enabled'], skill
                assert Path(skill['path']).resolve() == (path / 'skills/write-kiroha/SKILL.md').resolve(), skill
                assert not [e for item in response['data'] for e in item['errors']], response
                print(json.dumps({'codex': subprocess.check_output([args.codex, '--version'], text=True).strip(),
                    'marketplace': marketplace['marketplaceName'], 'plugin': plugin['pluginId'],
                    'version': plugin['version'], 'skill': skill['name'], 'enabled': skill['enabled'],
                    'bundle_bytes_match': True, 'discovery_errors': 0}, indent=2))
            finally:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
                reader.join(timeout=2)


if __name__ == '__main__':
    main()
