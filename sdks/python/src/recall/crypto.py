from __future__ import annotations

import hashlib
import os
from dataclasses import dataclass

from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)
from cryptography.hazmat.primitives.serialization import (
    Encoding,
    NoEncryption,
    PrivateFormat,
    PublicFormat,
)


@dataclass
class RecallKeypair:
    _private_key: Ed25519PrivateKey

    @classmethod
    def generate(cls) -> "RecallKeypair":
        return cls(_private_key=Ed25519PrivateKey.generate())

    @classmethod
    def from_bytes(cls, raw_bytes: bytes) -> "RecallKeypair":
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        key = Ed25519PrivateKey.from_private_bytes(raw_bytes)
        return cls(_private_key=key)

    def public_key_bytes(self) -> bytes:
        return self._private_key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)

    def sign(self, data: bytes) -> bytes:
        return self._private_key.sign(data)

    def private_bytes(self) -> bytes:
        return self._private_key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption())


def verify_signature(public_key_bytes: bytes, message: bytes, signature: bytes) -> bool:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    try:
        pub_key = Ed25519PublicKey.from_public_bytes(public_key_bytes)
        pub_key.verify(signature, message)
        return True
    except Exception:
        return False


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()
