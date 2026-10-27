#!/usr/bin/env python3
"""Validate and install the Claude Code plugin in disposable configuration.

No model calls are made; the user's Claude settings and installations are untouched.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
PLUGIN = ROOT / 'plugins/kiroha'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--claude', default='claude')
    parser.add_argument('--source', default=str(ROOT), help='local marketplace or GitHub repository')
    args = parser.parse_args()
    expected_version = json.loads((PLUGIN / 'plugin.json').read_text())['version']
    with tempfile.TemporaryDirectory(prefix='kiroha-claude.') as folder:
        env = dict(os.environ, CLAUDE_CONFIG_DIR=folder,
                   CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1',
                   CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL='1')
        def command(*arguments):
            result = subprocess.run([args.claude, *arguments], cwd=ROOT, env=env,
                                    capture_output=True, text=True, timeout=120)
            assert result.returncode == 0, result.stdout + result.stderr
            return result.stdout
        command('plugin', 'validate', '--strict', str(ROOT))
        command('plugin', 'validate', '--strict', str(PLUGIN))
        command('plugin', 'marketplace', 'add', args.source)
        command('plugin', 'install', 'kiroha@kiroha-lang', '--scope', 'user')
        listing = json.loads(command('plugin', 'list', '--json'))
        matches = [p for p in listing if p['id'] == 'kiroha@kiroha-lang']
        assert len(matches) == 1 and matches[0]['enabled'], listing
        assert matches[0]['version'] == expected_version, matches[0]
        installed = Path(matches[0]['installPath'])
        source_files = {p.relative_to(PLUGIN) for p in PLUGIN.rglob('*') if p.is_file()}
        installed_files = {p.relative_to(installed) for p in installed.rglob('*') if p.is_file()}
        assert source_files == installed_files, (source_files - installed_files, installed_files - source_files)
        for relative in source_files:
            assert (PLUGIN / relative).read_bytes() == (installed / relative).read_bytes(), relative
        details = command('plugin', 'details', 'kiroha@kiroha-lang')
        assert 'write-kiroha' in details and 'Skills (1)' in details, details
        version = subprocess.check_output([args.claude, '--version'], text=True).strip()
        print(json.dumps({'claude': version, 'plugin': matches[0]['id'],
                          'version': expected_version, 'enabled': True,
                          'skill': 'kiroha:write-kiroha',
                          'bundle_bytes_match': True}, indent=2))


if __name__ == '__main__':
    main()
