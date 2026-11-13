"""Archive contracts: no latest, no overwrite, exact bytes and safe failures."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publisher', Path(__file__).with_name('hfrog_publisher.py'))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class StorageError(Exception):
    def __init__(self, code):
        self.response = {'Error': {'Code': code}}


class ArchiveContracts(unittest.TestCase):
    def test_missing_version_is_distinct_from_business_errors(self):
        client = publisher.HfrogClient('https://api.invalid')
        for code, missing in ((1004, True), (1001, False)):
            body = io.BytesIO(json.dumps({'code': code, 'data': None}).encode())
            with patch.object(publisher.urllib.request, 'urlopen', return_value=body):
                if missing:
                    self.assertIsNone(client.get('/version', missing_ok=True))
                else:
                    with self.assertRaises(ValueError):
                        client.get('/version', missing_ok=True)

    def test_promoted_or_conflicting_releases_are_never_overwritten(self):
        asset = [('apk', Path('/file.apk'), 'https://cdn.invalid/file.apk', 3, 'sha')]
        row = {'platform_code': 'apk', 'download_url': 'https://cdn.invalid/file.apk',
               'file_size': 3, 'checksum_sha256': 'sha'}
        publisher.archive_record_gate({'is_latest': False, 'platforms': [row]}, asset, complete=True)
        for record in (None, {'is_latest': True, 'platforms': [row]},
                       {'is_latest': False, 'platforms': [dict(row, checksum_sha256='wrong')]},
                       {'is_latest': False, 'platforms': [row, row]}):
            with self.subTest(record=record), self.assertRaises(ValueError):
                publisher.archive_record_gate(record, asset, complete=True)

    def test_conditional_upload_reuses_only_identical_remote_bytes(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as folder:
            path = Path(folder) / 'file.apk'
            path.write_bytes(b'APK')
            objects = {}
            puts = []
            def get(**kwargs):
                if kwargs['Key'] not in objects:
                    raise StorageError('NoSuchKey')
                data = objects[kwargs['Key']]
                return {'ContentLength': len(data), 'Body': io.BytesIO(data)}
            def put(**kwargs):
                puts.append(kwargs['IfNoneMatch'])
                objects[kwargs['Key']] = kwargs['Body'].read()
            store = types.SimpleNamespace(get_object=get, put_object=put)
            for _ in range(2):
                publisher.immutable_put(store, 'bucket', path, 'version/file.apk', 'binary', 'immutable')
            self.assertEqual(puts, ['*'])
            objects['version/file.apk'] = b'WRONG'
            with self.assertRaises(ValueError):
                publisher.immutable_put(store, 'bucket', path, 'version/file.apk', 'binary', 'immutable')
            self.assertEqual(puts, ['*'])

    def test_conditional_race_still_requires_matching_bytes(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as folder:
            path = Path(folder) / 'file'
            path.write_bytes(b'ok')
            calls = [StorageError('NoSuchKey'), {'ContentLength': 3, 'Body': io.BytesIO(b'bad')}]
            def get(**kwargs):
                value = calls.pop(0)
                if isinstance(value, Exception):
                    raise value
                return value
            def put(**kwargs):
                raise StorageError('PreconditionFailed')
            with self.assertRaises(ValueError):
                publisher.immutable_put(types.SimpleNamespace(get_object=get, put_object=put),
                                        'bucket', path, 'key', 'binary', 'immutable')

    def test_download_requires_the_actual_size_and_hash(self):
        sha = hashlib.sha256(b'APK').hexdigest()
        for data in (b'BAD', b'APK-extra'):
            response = io.BytesIO(data)
            response.status = 200
            with patch.object(publisher.urllib.request, 'urlopen', return_value=response), self.assertRaises(ValueError):
                publisher.verify_download('https://cdn.invalid/file', 3, sha)
        response = io.BytesIO(b'APK')
        response.status = 200
        with patch.object(publisher.urllib.request, 'urlopen', return_value=response):
            publisher.verify_download('https://cdn.invalid/file', 3, sha)

    def test_archive_publishes_apk_aab_and_stable_manifest_separately(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as folder:
            root = Path(folder)
            apk, aab = root / 'game.apk', root / 'game.aab'
            apk.write_bytes(b'APK')
            aab.write_bytes(b'AAB')
            output = root / 'manifest.json'
            args = ['publish', '--archive', '--exact-version', '--tool', 'game', '--version', '1.2.3-dev.1.2',
                    '--r2-prefix', 'ci-archives/game', '--upload-r2', '--manifest-out', str(output),
                    '--asset', 'android-apk=' + str(apk), '--asset', 'android-aab=' + str(aab),
                    '--platform', 'android-apk={"os":"android","arch":"arm64","display_name":"APK"}',
                    '--platform', 'android-aab={"os":"android","arch":"arm64","display_name":"AAB"}',
                    '--git-commit', 'a' * 40, '--jenkins-url', 'https://ci.invalid/job/23/']
            rows, versions = [], []
            def post(endpoint, payload):
                if endpoint.endswith('/versions'):
                    versions.append(payload)
                if endpoint.endswith('/releases'):
                    rows.append(payload)
                return True, '{}'
            def get(endpoint, **kwargs):
                return None if kwargs.get('missing_ok') else {'is_latest': False, 'platforms': rows}
            client = types.SimpleNamespace(get=get, post=post)
            with patch.object(publisher, 'HfrogClient', return_value=client), \
                    patch.object(publisher, 'r2_put') as put, patch.object(publisher, 'verify_download') as verify:
                self.assertEqual(publisher.main(args), 0)
                first = output.read_bytes()
                self.assertEqual({r['platform_code'] for r in rows}, {'android-apk', 'android-aab', 'artifact-manifest'})
                self.assertEqual(versions[0]['version'], '1.2.3-dev.1.2')
                self.assertFalse(versions[0]['is_latest'])
                self.assertTrue(all(c.kwargs['immutable'] for c in put.call_args_list))
                self.assertEqual(verify.call_count, 3)
                self.assertEqual(len(json.loads(first)['artifacts']), 2)
                rows.clear()
                self.assertEqual(publisher.main(args), 0)
                self.assertEqual(output.read_bytes(), first)

    def test_archive_rejects_platform_collision_before_upload(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as folder:
            path = Path(folder) / 'file'
            path.write_bytes(b'file')
            with patch.object(publisher, 'r2_put') as put:
                result = publisher.main(['publish', '--archive', '--tool', 'game', '--version', '1.2.3',
                    '--r2-prefix', 'ci-archives/game', '--asset', 'x86_64-unknown-linux-gnu=' + str(path),
                    '--asset', 'x86_64-unknown-linux-gnu=' + str(path), '--upload-r2'])
                self.assertEqual(result, 1)
                put.assert_not_called()

    def test_archive_errors_never_echo_secret_bearing_exception_text(self):
        output = io.StringIO()
        with patch.object(publisher, 'cmd_publish', side_effect=RuntimeError('AUTH-TOKEN-MUST-NOT-APPEAR')), \
                contextlib.redirect_stdout(output):
            self.assertEqual(publisher.main(['publish', '--archive', '--tool', 'app',
                '--version', '1.2.3', '--r2-prefix', 'ci-archives/app']), 1)
        self.assertNotIn('AUTH-TOKEN', output.getvalue())

    def test_archive_rejects_different_platforms_sharing_one_object_key(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as folder:
            path = Path(folder) / 'same.apk'
            path.write_bytes(b'APK')
            with patch.object(publisher, 'r2_put') as put:
                self.assertEqual(publisher.main(['publish', '--archive', '--tool', 'game',
                    '--version', '1.2.3', '--r2-prefix', 'ci-archives/game', '--upload-r2',
                    '--asset', 'android-arm=' + str(path), '--asset', 'android-apk=' + str(path),
                    '--platform', 'android-apk={"os":"android","arch":"arm64","display_name":"APK"}']), 1)
                put.assert_not_called()

    def test_explicit_publish_keeps_latest_and_does_not_use_archive_gates(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as folder:
            path = Path(folder) / 'binary'
            path.write_bytes(b'bin')
            with patch.object(publisher, 'sync_hfrog_records', return_value=True) as sync, \
                    patch.object(publisher, 'archive_record_gate') as gate, \
                    patch.object(publisher, 'verify_download') as download:
                self.assertEqual(publisher.main(['publish', '--tool', 'app', '--version', '1.2.3',
                    '--r2-prefix', 'app', '--asset', 'x86_64-unknown-linux-gnu=' + str(path)]), 0)
                self.assertTrue(sync.call_args.kwargs['is_latest'])
                self.assertEqual(sync.call_args.kwargs['version'], 'v1.2.3')
                gate.assert_not_called()
                download.assert_not_called()


if __name__ == '__main__':
    unittest.main()
