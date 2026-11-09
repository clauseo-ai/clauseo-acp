#!/usr/bin/env python3
"""Writes fixtures.json in this folder: signed browser-pins manifests for the host's tests, each
an exact body and its minisign signature. Test data only: the keys are the block list's
throwaway test keys (crates/blocklist/tests/data, key A as "current", B as "next", C outside
every test set), whose seeds come from fixed phrases, so anyone can sign with them and no build
trusts them.

Run from this folder with Python 3 and the cryptography package: python3 make_fixtures.py
"""

import base64
import hashlib
import json

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

KEYS = "../../../../blocklist/tests/data"


def key(name):
    digest = hashlib.sha256(f"clauseo blocklist test key {name}".encode()).digest()
    private = Ed25519PrivateKey.from_private_bytes(digest)
    public = private.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    key_id = hashlib.sha256(public).digest()[:8]
    line = base64.b64encode(b"Ed" + key_id + public).decode()
    with open(f"{KEYS}/key-{name.lower()}.pub") as file:
        assert file.read().splitlines()[1] == line, f"key {name} differs from its .pub file"
    return private, key_id


def sign(name, data, trusted, prehashed=True):
    """A minisign signature over the BLAKE2b-512 of data (minisign's default, ED), or over data
    itself (the legacy Ed, which the app refuses)."""
    private, key_id = key(name)
    if prehashed:
        algorithm, signature = b"ED", private.sign(hashlib.blake2b(data, digest_size=64).digest())
    else:
        algorithm, signature = b"Ed", private.sign(data)
    global_signature = private.sign(signature + trusted.encode())
    return (
        "untrusted comment: signature from the browser pins test key\n"
        + base64.b64encode(algorithm + key_id + signature).decode()
        + f"\ntrusted comment: {trusted}\n"
        + base64.b64encode(global_signature).decode()
        + "\n"
    )


def fake_sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


# Chrome for Testing 157.0.8103.38's real hashes where known; made-up ones elsewhere.
CHROME = {
    "win64": {"sha256": "ae0deef27d449e1d7529df7705d609bab102de48464da13567ed3dd2ee1a0586",
              "size": 190000000},
    "mac-arm64": {"sha256": fake_sha("chrome mac-arm64"), "size": 180000000},
    "mac-x64": {"sha256": fake_sha("chrome mac-x64"), "size": 185000000},
    "linux64": {"sha256": "8cdeb344e9b2cefe59f4aea8d1d93d5d93054c03cde217665c16fc1f62e72a66",
                "size": 170000000},
}
SHELL = {
    "win64": {"sha256": "1ff4540855478ba0848e03f1ef8d455f77d058b38285df91bc34273788a73446",
              "size": 126513060},
    "mac-arm64": {"sha256": fake_sha("shell mac-arm64"), "size": 120000000},
    "mac-x64": {"sha256": fake_sha("shell mac-x64"), "size": 121000000},
    "linux64": {"sha256": "deafffa74660b34853a4119c361fd427ac8c9fcb62db51ddb8a797cd83a4ba2d",
                "size": 128168542},
}

# The zip the host's integration tests download: an empty zip, its end record only.
EMPTY_ZIP = bytes([0x50, 0x4B, 0x05, 0x06] + [0] * 18)


def body(published, version, chrome=None, schema=1, extra=True):
    chrome = CHROME if chrome is None else chrome
    manifest = {"schema": schema, "published": published}
    if extra:
        manifest["note"] = "fields the app does not know are ignored"
    stable = {"version": version, "downloads": {"chrome-headless-shell": SHELL, "chrome": chrome}}
    if extra:
        stable["revision"] = "1500000"
    manifest["stable"] = stable
    return json.dumps(manifest, separators=(",", ":"))


def comment(published, version, run="1234"):
    return f"clauseo-browser-pins v1 published={published} version={version} run={run}"


FIXTURES = {}


def fixture(name, text, trusted, signer="A", prehashed=True):
    FIXTURES[name] = {"body": text, "minisig": sign(signer, text.encode(), trusted, prehashed)}


P, V = "2026-11-07T06:00:00Z", "157.0.8103.38"
fixture("pins", body(P, V), comment(P, V))
fixture("pins-by-b", body(P, V), comment(P, V), signer="B")
fixture("pins-by-c", body(P, V), comment(P, V), signer="C")
fixture("pins-legacy", body(P, V), comment(P, V), prehashed=False)
fixture("pins-comment-published", body(P, V), comment("2026-11-07T06:00:01Z", V))
fixture("pins-comment-version", body(P, V), comment(P, "157.0.8103.39"))
fixture("pins-comment-no-run", body(P, V), f"clauseo-browser-pins v1 published={P} version={V}")
fixture("pins-comment-run-letters", body(P, V), comment(P, V, run="12a"))
fixture("pins-comment-run-empty", body(P, V), comment(P, V, run=""))
fixture("pins-comment-run-long", body(P, V), comment(P, V, run="1" * 21))
fixture("pins-comment-other", body(P, V), f"clauseo-blocklist v1 generated={P} entries=6")
fixture("pins-schema-2", body(P, V, schema=2), comment(P, V))
fixture("pins-no-extra", body(P, V, extra=False), comment(P, V))
for stem, pin in [
    ("sha-upper", {"sha256": CHROME["win64"]["sha256"].upper(), "size": 1}),
    ("sha-short", {"sha256": "ab" * 31, "size": 1}),
    ("size-zero", {"sha256": fake_sha("x"), "size": 0}),
    ("size-over", {"sha256": fake_sha("x"), "size": 419430401}),
]:
    fixture(f"pins-{stem}", body(P, V, chrome={**CHROME, "mac-x64": pin}), comment(P, V))
fixture("pins-version-bad", body(P, "157.0.x"), comment(P, "157.0.x"))
fixture("pins-newer", body("2026-11-08T06:00:00Z", "157.0.8103.50"),
        comment("2026-11-08T06:00:00Z", "157.0.8103.50", run="1240"))
fixture("pins-equal", body(P, V), comment(P, V, run="1239"))
fixture("pins-older", body("2026-11-06T00:00:00Z", V), comment("2026-11-06T00:00:00Z", V))
fixture("pins-lower", body("2026-11-08T12:00:00Z", "156.0.7000.1"),
        comment("2026-11-08T12:00:00Z", "156.0.7000.1"))
fixture("pins-ahead", body("2026-11-20T00:00:00Z", "157.0.8103.60"),
        comment("2026-11-20T00:00:00Z", "157.0.8103.60"))
# For the host's integration tests, whose clock starts at 2026-09-21T14:13:20Z: Chrome 131 on
# Apple silicon pinned to the empty zip, published 20 days before (old, not expired), about 44
# days before (still not expired) and about 46 days before (expired).
EMPTY = {"sha256": hashlib.sha256(EMPTY_ZIP).hexdigest(), "size": len(EMPTY_ZIP)}
for name, published, run in [
    ("pins-131", "2026-09-01T00:00:00Z", "7"),
    ("pins-131-44d", "2026-08-08T20:00:00Z", "9"),
    ("pins-131-46d", "2026-08-06T00:00:00Z", "10"),
]:
    fixture(name, body(published, "131.0.6778.85", chrome={**CHROME, "mac-arm64": EMPTY}),
            comment(published, "131.0.6778.85", run=run))
# Chrome 157 pinned 20 days before that clock: a downloaded 131 is out of date against it.
fixture("pins-157-early", body("2026-09-01T00:00:00Z", V),
        comment("2026-09-01T00:00:00Z", V, run="8"))
# Chrome 158, dated far ahead (a clock set back sees it as "ahead"), and a heartbeat: the same
# version as "pins", published a day later.
fixture("pins-158-ahead", body("2026-12-20T00:00:00Z", "158.0.8200.1"),
        comment("2026-12-20T00:00:00Z", "158.0.8200.1"))
fixture("pins-heartbeat", body("2026-11-08T06:00:00Z", V),
        comment("2026-11-08T06:00:00Z", V, run="1300"))

with open("fixtures.json", "w") as out:
    json.dump(FIXTURES, out, indent=1, sort_keys=True)
    out.write("\n")
