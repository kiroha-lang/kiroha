"""Code examples containing bracketed syntax must survive plugin link syncing."""
import importlib.util
from pathlib import Path
import unittest

SCRIPT = Path(__file__).resolve().parent.parent / 'scripts' / 'plugin.py'
spec = importlib.util.spec_from_file_location('kiroha_plugin', SCRIPT)
plugin = importlib.util.module_from_spec(spec)
spec.loader.exec_module(plugin)


class MarkdownLinks(unittest.TestCase):
    def test_sync_changes_only_prose_links(self):
        source = (
            '[guide](../guide.md), [`spec.md`](../spec.md), and `sql[Name](d){select 1}`\n'
            '``sql(d, q`select 1`)``\n'
            '```kiroha\n'
            'sql[Name](d){select ${name} as name}\n'
            '[link-like](value)\n'
            '```\n'
        )
        result = plugin.map_markdown_links(source, lambda m: f'[{m[1]}](absolute)')
        self.assertEqual(result, source.replace('[guide](../guide.md)', '[guide](absolute)')
                         .replace('[`spec.md`](../spec.md)', '[`spec.md`](absolute)'))

    def test_generated_database_reference_keeps_sql_example(self):
        reference = plugin.generated()[plugin.SKILL / 'references' / 'databases.md']
        self.assertIn('sql[Greeting](d){select', reference)
        self.assertNotIn('sql[Greeting](https://', reference)


if __name__ == '__main__':
    unittest.main()
