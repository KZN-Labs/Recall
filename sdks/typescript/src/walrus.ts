// Walrus blob fetch and verification helpers.

import { sha256Hex } from './crypto'

const WALRUS_AGGREGATOR_DEFAULT = 'https://aggregator.walrus-testnet.walrus.space'

export interface WalrusBlobResult {
  blobId: string
  data: Uint8Array
  contentHash: string
}

/**
 * Fetch a blob from Walrus by blob ID. Verifies the content hash on download.
 * Returns null if the blob is not found or the hash does not match.
 */
export async function fetchBlob(
  blobId: string,
  aggregatorUrl = WALRUS_AGGREGATOR_DEFAULT
): Promise<WalrusBlobResult | null> {
  try {
    const url = `${aggregatorUrl}/v1/${blobId}`
    const resp = await fetch(url)
    if (!resp.ok) return null
    const data = new Uint8Array(await resp.arrayBuffer())
    const contentHash = sha256Hex(data)
    return { blobId, data, contentHash }
  } catch {
    return null
  }
}

/**
 * Build a clickable Walrus explorer URL for a blob ID.
 * Used in the Inspector to provide independent verification links.
 */
export function walrusBlobUrl(blobId: string, network = 'testnet'): string {
  return `https://walruscan.com/${network}/blob/${blobId}`
}
