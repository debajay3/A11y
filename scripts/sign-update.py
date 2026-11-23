#!/usr/bin/env python3
"""Sign release checksums. The private key is provided only by Actions secrets."""
import base64
import os
import re
import sys
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

version = sys.argv[1].removeprefix('v')
if not version or any(c not in '0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-+' for c in version):
    raise SystemExit('Invalid release version')
root = Path(__file__).resolve().parent.parent
key = Ed25519PrivateKey.from_private_bytes(base64.b64decode(os.environ['A11Y_UPDATE_SIGNING_KEY'], validate=True))
public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
if public != base64.b64decode((root / 'crates/a11y-app/assets/update-public-key.txt').read_text().strip()):
    raise SystemExit('Signing secret does not match the public key embedded in A11y')
sums = root / 'dist/SHA256SUMS'
protocol = re.search(r'pub const PROTOCOL_VERSION: u32 = (\d+);', (root / 'crates/a11y-net/src/proto.rs').read_text()).group(1)
content = f'# a11y-version={version}\n# collaboration-protocol={protocol}\n'.encode() + sums.read_bytes()
sums.write_bytes(content)
sums.with_suffix('.sig').write_text(base64.b64encode(key.sign(content)).decode() + '\n')
print('Signed release manifest for', version)
