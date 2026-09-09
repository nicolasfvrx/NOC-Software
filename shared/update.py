"""Linux startup updater. Only the executable is replaced, never user data."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sys
import urllib.request

REPO = 'nicolasfvrx/NOC-Software'
API = 'https://api.github.com/repos/' + REPO
MAX_SIZE = 128 * 1024 * 1024


def version(value):
    match = re.fullmatch(r'v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', value)
    return tuple(map(int, match.groups())) if match else None


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not newurl.startswith('https://'):
            raise ValueError('Non-HTTPS redirect')
        result = super().redirect_request(req, fp, code, msg, headers, newurl)
        if result is not None:
            result.remove_header('Authorization')
        return result


def fetch(url, limit, binary=False):
    # URLs are constructed locally; the API response cannot choose another origin.
    headers = {'User-Agent': 'NOC-Startup-Updater', 'Accept': 'application/octet-stream' if binary else 'application/vnd.github+json'}
    token = os.environ.get('NOC_GITHUB_TOKEN')
    if token:
        headers['Authorization'] = 'Bearer ' + token
    with urllib.request.build_opener(SafeRedirect()).open(urllib.request.Request(url, headers=headers), timeout=12) as response:
        content = response.read(limit + 1)
        if len(content) > limit:
            raise ValueError('Download too large')
        return content


def main():
    app = os.environ['NOC_UPDATE_APP']
    target = Path(os.environ['NOC_UPDATE_EXE'])
    stage = Path(os.environ['NOC_UPDATE_STAGE'])
    current = version(os.environ['NOC_UPDATE_CURRENT'])
    asset_name = os.environ['NOC_UPDATE_ASSET']
    if not current:
        return 0
    with (target.parent / '.noc-updates' / (app + '.lock')).open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        initial = target.stat()
        release = json.loads(fetch(API + '/releases/latest', 2 * 1024 * 1024))
        remote = version(release.get('tag_name', ''))
        if release.get('draft') or release.get('prerelease') or not remote or remote <= current:
            return 0
        matches = [a for a in release.get('assets', []) if a.get('name') == asset_name and a.get('state') == 'uploaded']
        if len(matches) != 1:
            raise ValueError('No matching release asset')
        asset = matches[0]
        if not isinstance(asset.get('id'), int) or not 0 < asset.get('size', 0) <= MAX_SIZE:
            raise ValueError('Invalid asset')
        content = fetch(API + '/releases/assets/' + str(asset['id']), MAX_SIZE, True)
        if len(content) != asset['size'] or 'sha256:' + hashlib.sha256(content).hexdigest() != asset.get('digest'):
            raise ValueError('Executable checksum mismatch or unavailable')
        if content[:5] != b'\x7fELF\x02' or content[18:20] != b'\x3e\x00':
            raise ValueError('Not a Linux x64 executable')
        candidate = stage / 'candidate'
        candidate.write_bytes(content)
        # Preserve the installed executable's access mode and ownership. No chmod 777.
        os.chmod(candidate, initial.st_mode & 0o777)
        if (candidate.stat().st_uid, candidate.stat().st_gid) != (initial.st_uid, initial.st_gid):
            os.chown(candidate, initial.st_uid, initial.st_gid)
        if target.stat().st_ino != initial.st_ino:
            raise ValueError('Executable changed during update')
        backup = target.parent / '.noc-updates' / (app + '.previous')
        shutil.copy2(target, stage / 'previous')
        os.replace(stage / 'previous', backup)
        os.replace(candidate, target)
        return 20


if __name__ == '__main__':
    try:
        sys.exit(main())
    except Exception as error:
        # Avoid logging URLs, tokens or application arguments.
        try:
            path = Path(os.environ['NOC_UPDATE_EXE']).parent / '.noc-updates' / (os.environ['NOC_UPDATE_APP'] + '.log')
            with path.open('a') as log:
                log.write('Update skipped: ' + type(error).__name__ + '\n')
        except OSError:
            pass
        sys.exit(0)
