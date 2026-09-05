#!/bin/sh
# Fetch 26.2 references to CACHE ONLY. Never writes into the repo.
# Usage: sh scripts/fetch-26.2.sh [cache_dir]
set -eu
CACHE="${1:-${XDG_CACHE_HOME:-$HOME/.cache}/mc-rust-client/26.2}"
mkdir -p "$CACHE"
echo "cache: $CACHE"

manifest_url="https://piston-meta.mojang.com/v1/packages/3592ebc61c6b6c33bb8228fe5a9e90221df0be68/26.2.json"
curl -fsSL "$manifest_url" -o "$CACHE/26.2.json"
echo "saved $CACHE/26.2.json"

python3 - "$CACHE/26.2.json" "$CACHE" <<'PY'
import json, sys, urllib.request, hashlib, os
mp, cache = sys.argv[1], sys.argv[2]
d = json.load(open(mp))
for side in ("client", "server"):
    info = d["downloads"][side]
    dest = os.path.join(cache, side + ".jar")
    if os.path.exists(dest):
        h = hashlib.sha1(open(dest, "rb").read()).hexdigest()
        if h == info["sha1"]:
            print(f"{side}: cached OK {h}")
            continue
        print(f"{side}: hash mismatch, re-downloading")
    print(f"{side}: downloading {info['url']}")
    urllib.request.urlretrieve(info["url"], dest)
    h = hashlib.sha1(open(dest, "rb").read()).hexdigest()
    assert h == info["sha1"], f"{side} SHA1 mismatch: {h} != {info['sha1']}"
    print(f"{side}: OK {h} ({info['size']} bytes)")
PY
echo "done. Decompile locally for reading only, e.g.:"
echo "  java -jar vineflower.jar $CACHE/client.jar $CACHE/decompiled/"
echo "Do NOT copy output into the repo (see docs/LEGAL.md)."
