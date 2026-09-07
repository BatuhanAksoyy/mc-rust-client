"""Prepare verified Minecraft resources and the block-state registry locally."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def pinned_metadata():
    source = (ROOT / 'crates/mc-launcher/src/lib.rs').read_text()
    def constant(name):
        return re.search(r'pub const ' + name + r': &str =\s*"([^"]+)";', source).group(1)
    url = constant('VERSION_26_2_URL')
    return url, url.split('/')[-2], {
        side: constant(side.upper() + '_SHA1_26_2') for side in ('client', 'server')
    }


def digest(path):
    result = hashlib.sha1()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()


def fetch(url, destination, expected):
    if destination.is_symlink():
        raise RuntimeError(f'Refusing symlink: {destination}')
    if destination.is_file() and digest(destination) == expected:
        print(f'Using verified {destination.name}', flush=True)
        return
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as output:
            temporary = Path(output.name)
            print(f'Downloading {destination.name}', flush=True)
            with urllib.request.urlopen(url, timeout=60) as response:
                shutil.copyfileobj(response, output)
        if digest(temporary) != expected:
            raise RuntimeError(f'SHA-1 mismatch for {destination.name}; rerun to retry')
        temporary.replace(destination)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def extract_resources(archive, destination):
    with zipfile.ZipFile(archive) as jar:
        for member in jar.infolist():
            path = PurePosixPath(member.filename)
            if not path.parts or path.parts[0] != 'assets':
                continue
            if ('..' in path.parts or '\\' in member.filename or ':' in member.filename
                    or path.suffix == '.class'):
                raise RuntimeError(f'Invalid resource path: {member.filename}')
            target = destination.joinpath(*path.parts)
            if member.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with jar.open(member) as source, target.open('wb') as output:
                    shutil.copyfileobj(source, output)
    root = destination / 'assets' / 'minecraft'
    if not all((root / name).is_dir() for name in ('textures', 'models', 'blockstates')):
        raise RuntimeError('Client archive is missing required block resources')


def registry_report(blocks):
    by_id = {}
    for name, block in blocks.items():
        for state in block['states']:
            index = state['id']
            if type(index) is not int or index < 0 or index in by_id:
                raise RuntimeError('Invalid or duplicate block-state ID')
            by_id[index] = {'name': name, 'properties': state.get('properties', {})}
    if not by_id or set(by_id) != set(range(len(by_id))):
        raise RuntimeError('Block-state IDs must be nonempty and contiguous')
    return {'protocol': 776, 'version': '26.2',
            'states': [by_id[index] for index in range(len(by_id))]}


def local_cache(cwd):
    return cwd / '.data' / 'mc-rust-client' / '26.2'


def ready(cache):
    return ((cache / 'block-states.json').is_file()
            and (cache / 'client-extracted/assets/minecraft').is_dir())


def main():
    parser = argparse.ArgumentParser(description='Download block textures and generate the registry '
                                     'in CWD/.data. Requires Python 3 and Java (setup only).')
    parser.parse_args()
    java = shutil.which('java')
    if java is None:
        raise RuntimeError('Java is required to generate block-states.json. Install Java '
                           'for Minecraft 26.2, ensure java is on PATH, then rerun download-assets.')
    cache = local_cache(Path.cwd())
    for path in [cache, *cache.parents]:
        if path.is_symlink():
            raise RuntimeError(f'Refusing symlinked asset directory: {path}')
    cache.mkdir(parents=True, exist_ok=True)
    url, manifest_hash, hashes = pinned_metadata()
    manifest_path = cache / '26.2.json'
    fetch(url, manifest_path, manifest_hash)
    metadata = json.loads(manifest_path.read_text())
    if metadata['id'] != '26.2':
        raise RuntimeError('Unexpected Minecraft version metadata')
    print(f"Setup requires Java {metadata['javaVersion']['majorVersion']} or newer", flush=True)
    for side, expected in hashes.items():
        info = metadata['downloads'][side]
        if info['sha1'] != expected:
            raise RuntimeError(f'Pinned {side} checksum disagrees with metadata')
        fetch(info['url'], cache / (side + '.jar'), expected)
    with tempfile.TemporaryDirectory(prefix='asset-setup-', dir=cache) as temporary:
        work = Path(temporary)
        extracted = work / 'client-extracted'
        extract_resources(cache / 'client.jar', extracted)
        print('Generating block-state registry with Java...', flush=True)
        subprocess.run([java, '-DbundlerMainClass=net.minecraft.data.Main', '-jar',
                        str(cache / 'server.jar'), '--reports'], cwd=work, check=True)
        blocks = json.loads((work / 'generated/reports/blocks.json').read_text())
        report = work / 'block-states.json'
        report.write_text(json.dumps(registry_report(blocks)), encoding='utf-8')
        destination = cache / 'client-extracted'
        registry = cache / 'block-states.json'
        if destination.is_symlink() or registry.is_symlink():
            raise RuntimeError('Refusing symlinked asset output')
        if destination.exists():
            shutil.rmtree(destination)
        extracted.replace(destination)
        report.replace(registry)
    print(f'Assets ready in {cache}. Run launch-client from this working directory.')
    return 0


def entry():
    try:
        return main()
    except KeyboardInterrupt:
        return 130
    except (OSError, RuntimeError, ValueError, KeyError, zipfile.BadZipFile,
            subprocess.CalledProcessError) as error:
        print(f'download-assets: {error}', file=sys.stderr)
        return 1
