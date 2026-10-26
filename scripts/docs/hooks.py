"""Publish repository Markdown without keeping a second copy of the reference."""
from pathlib import Path
import os
import re
import tomllib
from urllib.parse import unquote, urlsplit

from mkdocs.structure.files import File
from pygments import highlight
from pygments.formatters import HtmlFormatter
from pygments.lexer import RegexLexer, bygroups, words
from pygments.token import Comment, Keyword, Name, Number, Operator, Punctuation, String, Text

ROOT = Path(__file__).resolve().parents[2]
VERSION = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
REPO = 'https://github.com/kiroha-lang/kiroha'
_block = 0


class KirohaLexer(RegexLexer):
    name = 'Kiroha'
    tokens = {
        'root': [
            (r'//[^\n]*', Comment.Single),
            (r'"', String.Double, 'string'),
            (r'q`', String.Backtick, 'query'),
            (words(('fun', 'then', 'end', 'if', 'else', 'do', 'try', 'catch'), suffix=r'\b'), Keyword),
            (words(('null', 'true', 'false'), suffix=r'\b'), Keyword.Constant),
            (r'\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?', Number),
            (r'([A-Za-z_]\w*)(\s*)(?=\()', bygroups(Name.Function, Text)),
            (r'[A-Za-z_]\w*', Name),
            (r'=>|==|!=|<=|>=|&&|\|\||\?\?|[+*/%!=<>|?-]', Operator),
            (r'[(){}\[\],;:.]', Punctuation),
            (r'\s+|.', Text),
        ],
        'string': [(r'\\(?:u[0-9a-fA-F]{4}|.)', String.Escape), (r'"', String.Double, '#pop'), (r'[^"\\]+', String.Double)],
        'query': [(r'\\.', String.Escape), (r'`', String.Backtick, '#pop'), (r'[^`\\]+', String.Backtick)],
    }


def kiroha_code(source, language, class_name, options, md, **kwargs):
    global _block
    _block += 1
    code = highlight(source, KirohaLexer(), HtmlFormatter(nowrap=True))
    return f'<div class="language-kiroha highlight"><pre id="kiroha-code-{_block}"><span></span><code>{code}</code></pre></div>'


def on_config(config):
    config.mdx_configs.setdefault('pymdownx.superfences', {})['custom_fences'] = [
        {'name': 'kiroha', 'class': 'language-kiroha', 'format': kiroha_code}
    ]
    return config


def on_files(files, config):
    spec = File.generated(config, 'spec.md', abs_src_path=str(ROOT / 'spec.md'))
    spec.edit_uri = '../spec.md'
    files.append(spec)
    examples = File.generated(config, 'examples.md', abs_src_path=str(ROOT / 'examples/README.md'))
    examples.edit_uri = '../examples/README.md'
    files.append(examples)
    for name in ('favicon.ico', 'favicon-16.png', 'favicon-32.png'):
        files.append(File.generated(config, f'assets/{name}',
                                    abs_src_path=str(ROOT / 'assets/branding' / name)))
    return files


def on_page_markdown(markdown, page, config, files):
    global _block
    _block = 0
    # Keep formal and dated evidence available without drowning out tutorials
    # in search results with the machine-readable conformance corpus.
    if page.file.src_uri in ('spec.md', 'completion.md', 'conformance-report.md', 'naming.md'):
        page.meta['search'] = {'exclude': True}
    markdown = markdown.replace('{{ kiroha_version }}', VERSION)
    source = Path(page.file.abs_src_path)
    by_path = {Path(f.abs_src_path).resolve(): f.src_uri for f in files if f.abs_src_path}

    def link(match):
        url = match.group(2)
        parsed = urlsplit(url)
        if parsed.scheme or parsed.netloc or url.startswith('#'):
            return match.group(0)
        target = (source.parent / unquote(parsed.path)).resolve()
        if target in by_path:
            dest = os.path.relpath(by_path[target], str(Path(page.file.src_uri).parent))
            if parsed.fragment:
                dest += '#' + parsed.fragment
            return match.group(1) + dest + match.group(3)
        if target.is_relative_to(ROOT) and target.exists():
            kind = 'tree' if target.is_dir() else 'blob'
            dest = f'{REPO}/{kind}/main/{target.relative_to(ROOT).as_posix()}'
            if parsed.fragment:
                dest += '#' + parsed.fragment
            return match.group(1) + dest + match.group(3)
        return match.group(0)

    # Skip fenced and inline code: a Markdown-looking string inside an example
    # must remain literal. MkDocs converts the rewritten .md links to page URLs.
    out = []
    fence = None
    for line in markdown.splitlines(keepends=True):
        marker = re.match(r'^\s*(`{3,}|~{3,})', line)
        if marker:
            token = marker[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            out.append(line)
        elif fence:
            out.append(line)
        else:
            parts = re.split(r'(`+[^`]*`+)', line)
            out.append(''.join(part if i % 2 else re.sub(r'(\]\()([^\s)]+)(\))', link, part)
                               for i, part in enumerate(parts)))
    return ''.join(out)
