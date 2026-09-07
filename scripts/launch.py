"""Shared implementation for the repository's Python 3 launch scripts."""

import hashlib
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.parse
import urllib.request

from scripts.assets import local_cache, ready

ROOT = Path(__file__).resolve().parents[1]


def release_asset():
    """Read the launcher's pinned release metadata, keeping one source of truth."""
    source = (ROOT / 'crates/mc-launcher/src/pumpkin/release.rs').read_text()
    release = re.search(r'pub const RELEASE: &str = "([^"]+)";', source).group(1)
    system = {'Darwin': 'macos', 'Linux': 'linux', 'Windows': 'windows'}.get(platform.system())
    machine = platform.machine().lower()
    arch = {'amd64': 'x86_64', 'arm64': 'aarch64'}.get(machine, machine)
    assets = re.findall(
        r'\("([^"]+)", "([^"]+)"\) => \(\s*"([^"]+)",\s*"([a-f0-9]{64})",', source
    )
    for os_name, architecture, name, digest in assets:
        if (os_name, architecture) == (system, arch):
            return release, name, digest
    raise RuntimeError(f'No pinned Pumpkin executable for {platform.system()} {machine}')


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def download(directory):
    release, name, digest = release_asset()
    if directory.is_symlink() or directory.parent.is_symlink():
        raise RuntimeError(f'Refusing symlinked download directory: {directory}')
    directory.mkdir(parents=True, exist_ok=True)
    binary = directory / name
    if binary.is_symlink():
        raise RuntimeError(f'Refusing symlink: {binary}')
    if binary.is_file() and sha256(binary) == digest:
        binary.chmod(0o755)
        return binary
    url = ('https://github.com/Pumpkin-MC/Pumpkin/releases/download/'
           + urllib.parse.quote(release, safe='') + '/' + name)
    print(f'Downloading {name} into {directory}', flush=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=directory, delete=False) as output:
            temporary = Path(output.name)
            with urllib.request.urlopen(url, timeout=60) as response:
                shutil.copyfileobj(response, output)
        if sha256(temporary) != digest:
            raise RuntimeError('Pumpkin download failed SHA-256 verification; rerun to retry')
        temporary.chmod(0o755)
        temporary.replace(binary)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return binary


def launch_environment(mode, cwd):
    environment = os.environ.copy()
    if mode == 'client' and ready(local_cache(cwd)):
        environment['XDG_CACHE_HOME'] = str(cwd / '.data')
    return environment


def main(mode):
    args = sys.argv[1:]
    if '--help' in args or '-h' in args:
        print(f'Usage: launch-{mode} [options]')
        if mode == 'server':
            print('Downloads Pumpkin to CWD/.data/bin; world/logs: CWD/.data/server.\n'
                  'Options: --port PORT, --startup-seconds SECONDS, --check')
        else:
            print('Options: [HOST], --port PORT, --name NAME, --timeout-ms MS,\n'
                  '         --render-distance CHUNKS\n'
                  'Start launch-server first. HOST defaults to localhost.')
        return 0
    if shutil.which('cargo') is None:
        raise RuntimeError('cargo is required; install the Rust toolchain before launching')
    if mode == 'server':
        if any(arg.split('=')[0] in ('--session', '--pumpkin') for arg in args):
            raise RuntimeError('launch-server manages --session and --pumpkin in CWD/.data')
        data = Path.cwd() / '.data'
        binary = download(data / 'bin')
        command = ['local', '--pumpkin', str(binary), '--session', str(data / 'server')]
    else:
        command = ['render']

    # Explicit target directory makes launch independent of CARGO_TARGET_DIR and CWD.
    target = ROOT / 'target'
    subprocess.run(['cargo', 'build', '--locked', '--release', '-p', 'mc-client',
                    '--target-dir', str(target)], cwd=ROOT, check=True)
    executable = target / 'release' / ('mc-client.exe' if os.name == 'nt' else 'mc-client')
    environment = launch_environment(mode, Path.cwd())
    print(f'Launching {mode}', flush=True)
    # On Unix replacement preserves direct Ctrl-C delivery to the Rust process.
    if os.name != 'nt':
        os.execve(str(executable), [str(executable), *command, *args], environment)
    child = subprocess.Popen([str(executable), *command, *args], env=environment)
    while True:
        try:
            return child.wait()
        except KeyboardInterrupt:
            # The console delivers Ctrl-C to the child too; let it save and exit.
            continue


def entry(mode):
    try:
        return main(mode)
    except KeyboardInterrupt:
        return 130
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f'launch-{mode}: {error}', file=sys.stderr)
        return 1
