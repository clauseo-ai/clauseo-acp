#!/usr/bin/env python3
"""Writes the block list test fixtures in this folder: three throwaway minisign keys (A and B in
the tests' key set, C outside it) and the signed lists the tests read. Test data only: the keys'
seeds come from fixed phrases, so anyone can sign with them, and no build trusts them.

Run from this folder with Python 3 and the `cryptography` package: python3 make_fixtures.py
"""

import base64
import gzip
import hashlib

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


def key(name):
    digest = hashlib.sha256(f"clauseo blocklist test key {name}".encode()).digest()
    private = Ed25519PrivateKey.from_private_bytes(digest)
    public = private.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    key_id = hashlib.sha256(public).digest()[:8]
    return private, public, key_id


def write_public(name):
    _, public, key_id = key(name)
    line = base64.b64encode(b"Ed" + key_id + public).decode()
    hex_id = key_id[::-1].hex().upper()
    with open(f"key-{name.lower()}.pub", "w") as out:
        out.write(f"untrusted comment: minisign public key {hex_id}\n{line}\n")


def sign(name, data, trusted, prehashed=True):
    """A minisign signature over the BLAKE2b-512 of `data` (minisign's default, `ED`), or over
    `data` itself (the legacy `Ed`, which the app refuses)."""
    private, _, key_id = key(name)
    if prehashed:
        algorithm, signature = b"ED", private.sign(hashlib.blake2b(data, digest_size=64).digest())
    else:
        algorithm, signature = b"Ed", private.sign(data)
    global_signature = private.sign(signature + trusted.encode())
    return (
        "untrusted comment: signature from the block list test key\n"
        + base64.b64encode(algorithm + key_id + signature).decode()
        + f"\ntrusted comment: {trusted}\n"
        + base64.b64encode(global_signature).decode()
        + "\n"
    )


HOSTS = [
    ("1.2.3.4", "m"),
    ("[2001:db8::1]", "m"),
    ("evil.example", "p"),
    ("login.bank-secure.test", "p"),
    ("phish.test", "p"),
    ("xn--80ak6aa92e.com", "p"),
]


def fixture(stem, generated, hosts, signer="A", entries=None, prehashed=True):
    lines = [
        "# clauseo-blocklist v1",
        f"# generated: {generated}",
        "# sources: urlhaus=none; phishing-database=0123abcd; feodo=2026-11-07T05:00:00Z",
    ] + [f"{host} {tag}" for host, tag in sorted(hosts)]
    data = gzip.compress(("\n".join(lines) + "\n").encode(), mtime=0)
    count = len(hosts) if entries is None else entries
    trusted = f"clauseo-blocklist v1 generated={generated} entries={count}"
    with open(f"{stem}.txt.gz", "wb") as out:
        out.write(data)
    with open(f"{stem}.txt.gz.minisig", "w") as out:
        out.write(sign(signer, data, trusted, prehashed))


for name in "ABC":
    write_public(name)
fixture("list", "2026-11-07T06:00:00Z", HOSTS)
fixture("list-newer", "2026-11-07T12:00:00Z", HOSTS + [("newer.test", "p")])
fixture("list-older", "2026-11-06T00:00:00Z", HOSTS[:3])
fixture("list-by-b", "2026-11-07T06:00:00Z", HOSTS, signer="B")
fixture("list-by-c", "2026-11-07T06:00:00Z", HOSTS, signer="C")
fixture("list-miscounted", "2026-11-07T06:00:00Z", HOSTS, entries=len(HOSTS) + 1)
fixture("list-legacy", "2026-11-07T06:00:00Z", HOSTS, prehashed=False)
