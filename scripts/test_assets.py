"""Synthetic asset setup tests: no proprietary fixtures or network required."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import assets
import launch


class AssetTests(unittest.TestCase):
    def test_checksum_reuse_and_failed_repair_preserves_file(self):
        payload = b'synthetic archive'
        with tempfile.TemporaryDirectory() as root:
            target = Path(root) / 'client.jar'
            with patch('urllib.request.urlopen', return_value=io.BytesIO(payload)) as request:
                expected = hashlib.sha1(payload).hexdigest()
                assets.fetch('https://example.invalid', target, expected)
                assets.fetch('https://example.invalid', target, expected)
                self.assertEqual(request.call_count, 1)
            with patch('urllib.request.urlopen', return_value=io.BytesIO(b'bad')):
                with self.assertRaisesRegex(RuntimeError, 'SHA-1'):
                    assets.fetch('https://example.invalid', target, '0' * 40)
            self.assertEqual(target.read_bytes(), payload)
            self.assertEqual(list(Path(root).iterdir()), [target])

    def test_extraction_filters_classes_and_rejects_traversal(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            archive = root / 'client.jar'
            with zipfile.ZipFile(archive, 'w') as jar:
                jar.writestr('net/example/Test.class', b'synthetic')
                for kind in ['textures', 'models', 'blockstates']:
                    jar.writestr(f'assets/minecraft/{kind}/test.json', '{}')
            assets.extract_resources(archive, root / 'out')
            self.assertFalse((root / 'out/net').exists())
            with zipfile.ZipFile(archive, 'w') as jar:
                jar.writestr('assets/../../escape', b'bad')
            with self.assertRaisesRegex(RuntimeError, 'Invalid resource path'):
                assets.extract_resources(archive, root / 'bad')
            self.assertFalse((root / 'escape').exists())

    def test_registry_orders_ids_and_keeps_properties(self):
        report = assets.registry_report({
            'minecraft:stone': {'states': [{'id': 1, 'properties': {'test': 'value'}}]},
            'minecraft:air': {'states': [{'id': 0}]},
        })
        self.assertEqual(report['states'][0]['name'], 'minecraft:air')
        self.assertEqual(report['states'][1]['properties'], {'test': 'value'})
        with self.assertRaisesRegex(RuntimeError, 'contiguous'):
            assets.registry_report({'x': {'states': [{'id': 1}]}})

    def test_setup_and_client_cache_selection(self):
        with tempfile.TemporaryDirectory(prefix='asset test ') as root:
            root = Path(root)
            def fetch(url, target, expected):
                if target.suffix == '.json':
                    target.write_text(json.dumps({'id': '26.2', 'javaVersion': {'majorVersion': 25},
                        'downloads': {side: {'sha1': side, 'url': side} for side in ['client', 'server']}}))
                elif target.name == 'client.jar':
                    with zipfile.ZipFile(target, 'w') as jar:
                        for kind in ['textures', 'models', 'blockstates']:
                            jar.writestr(f'assets/minecraft/{kind}/test.json', '{}')
                else:
                    target.write_bytes(b'synthetic')
            def generate(command, cwd, check):
                self.assertIn('--reports', command)
                self.assertTrue(check)
                output = cwd / 'generated/reports'
                output.mkdir(parents=True)
                (output / 'blocks.json').write_text(json.dumps({'minecraft:air': {'states': [{'id': 0}]}}))
            with patch('sys.argv', ['download-assets']), patch.object(Path, 'cwd', return_value=root), \
                    patch('shutil.which', return_value='java'), patch.object(assets, 'fetch', side_effect=fetch), \
                    patch.object(assets, 'pinned_metadata', return_value=('url', 'hash', {'client': 'client', 'server': 'server'})), \
                    patch('subprocess.run', side_effect=generate), \
                    patch.dict('os.environ', {'XDG_CACHE_HOME': 'original'}):
                self.assertEqual(launch.launch_environment('client', root)['XDG_CACHE_HOME'], 'original')
                self.assertEqual(assets.main(), 0)
                self.assertTrue(assets.ready(assets.local_cache(root)))
                self.assertEqual(launch.launch_environment('client', root)['XDG_CACHE_HOME'], str(root / '.data'))
                self.assertEqual(launch.launch_environment('server', root)['XDG_CACHE_HOME'], 'original')
                self.assertEqual(assets.main(), 0)
