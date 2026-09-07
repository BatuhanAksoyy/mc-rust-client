"""Offline regression tests for launch downloads; run with unittest discovery."""
import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import launch


class DownloadTests(unittest.TestCase):
    def test_all_supported_platforms_use_pinned_metadata(self):
        for system, machine in [('Linux', 'x86_64'), ('Linux', 'aarch64'),
                                ('Darwin', 'arm64'), ('Windows', 'AMD64'),
                                ('Windows', 'ARM64')]:
            with self.subTest(system=system, machine=machine), \
                    patch('platform.system', return_value=system), \
                    patch('platform.machine', return_value=machine):
                release, name, digest = launch.release_asset()
                self.assertEqual(release, '0.1.0-dev+26.2-26.45')
                self.assertTrue(name.startswith('pumpkin-'))
                self.assertEqual(len(digest), 64)

    def test_download_verify_reuse_and_repair(self):
        payload = b'synthetic executable'
        digest = hashlib.sha256(payload).hexdigest()
        with tempfile.TemporaryDirectory(prefix='launch test ') as root, \
                patch.object(launch, 'release_asset', return_value=('test', 'pumpkin', digest)), \
                patch('urllib.request.urlopen', side_effect=lambda *a, **k: io.BytesIO(payload)) as fetch:
            directory = Path(root) / '.data' / 'bin'
            binary = launch.download(directory)
            self.assertEqual(binary.read_bytes(), payload)
            self.assertEqual(launch.download(directory), binary)
            self.assertEqual(fetch.call_count, 1)
            binary.write_bytes(b'corrupt')
            launch.download(directory)
            self.assertEqual(binary.read_bytes(), payload)
            self.assertEqual(fetch.call_count, 2)

    def test_bad_download_does_not_replace_existing_file(self):
        with tempfile.TemporaryDirectory() as root, \
                patch.object(launch, 'release_asset', return_value=('test', 'pumpkin', '0' * 64)), \
                patch('urllib.request.urlopen', return_value=io.BytesIO(b'wrong')):
            directory = Path(root)
            binary = directory / 'pumpkin'
            binary.write_bytes(b'original')
            with self.assertRaisesRegex(RuntimeError, 'SHA-256'):
                launch.download(directory)
            self.assertEqual(binary.read_bytes(), b'original')
            self.assertEqual(list(directory.iterdir()), [binary])

    def test_help_needs_no_cargo_or_download(self):
        for mode in ['server', 'client']:
            with patch('sys.argv', ['launch-' + mode, '--help']), \
                    patch.object(launch, 'download') as download, \
                    patch('subprocess.run') as build:
                self.assertEqual(launch.main(mode), 0)
                download.assert_not_called()
                build.assert_not_called()
