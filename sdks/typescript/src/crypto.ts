// Ed25519 cryptography for the browser SDK using noble-curves.
// @noble/curves is the canonical choice for zero-dependency Ed25519 in browsers.

import { sha256 } from '@noble/hashes/sha256'
import { bytesToHex, hexToBytes } from '@noble/hashes/utils'
import { ed25519 } from '@noble/curves/ed25519'

export interface Keypair {
  publicKey: Uint8Array
  privateKey: Uint8Array
}

export function generateKeypair(): Keypair {
  const privateKey = ed25519.utils.randomPrivateKey()
  const publicKey = ed25519.getPublicKey(privateKey)
  return { publicKey, privateKey }
}

export function sign(message: Uint8Array, privateKey: Uint8Array): Uint8Array {
  return ed25519.sign(message, privateKey)
}

export function verify(
  message: Uint8Array,
  signature: Uint8Array,
  publicKey: Uint8Array
): boolean {
  try {
    return ed25519.verify(signature, message, publicKey)
  } catch {
    return false
  }
}

export function sha256Hex(data: Uint8Array): string {
  return bytesToHex(sha256(data))
}

export function sha256Bytes(data: Uint8Array): Uint8Array {
  return sha256(data)
}
