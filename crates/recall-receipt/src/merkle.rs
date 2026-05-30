use recall_core::ids::ContentHash;
use recall_crypto::sha256_bytes;

/// Compute the Merkle root of a list of receipt IDs (SHA-256 hashes).
/// Uses a simple binary Merkle tree: each leaf is the raw SHA-256 hash bytes.
/// If the list has an odd number of elements, the last element is duplicated.
pub fn merkle_root(receipt_ids: &[ContentHash]) -> ContentHash {
    if receipt_ids.is_empty() {
        return ContentHash(hex::encode([0u8; 32]));
    }

    let mut level: Vec<[u8; 32]> = receipt_ids
        .iter()
        .map(|id| {
            let mut arr = [0u8; 32];
            let bytes = hex::decode(&id.0).expect("valid hex");
            arr.copy_from_slice(&bytes);
            arr
        })
        .collect();

    // A single leaf is treated as odd-length: duplicate it before hashing.
    if level.len() == 1 {
        level.push(level[0]);
    }

    while level.len() > 1 {
        let mut next_level = Vec::new();
        let mut i = 0;
        while i < level.len() {
            let left = level[i];
            let right = if i + 1 < level.len() {
                level[i + 1]
            } else {
                level[i] // duplicate last if odd
            };
            let mut combined = [0u8; 64];
            combined[..32].copy_from_slice(&left);
            combined[32..].copy_from_slice(&right);
            next_level.push(sha256_bytes(&combined));
            i += 2;
        }
        level = next_level;
    }

    ContentHash(hex::encode(level[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_merkle_root_is_zeroes() {
        let root = merkle_root(&[]);
        assert_eq!(root.0, "0".repeat(64));
    }

    #[test]
    fn single_entry_merkle_root() {
        let id = ContentHash("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string());
        let root = merkle_root(&[id.clone()]);
        // Single entry: root = SHA-256(id || id)
        let mut combined = [0u8; 64];
        let bytes = hex::decode(&id.0).unwrap();
        combined[..32].copy_from_slice(&bytes);
        combined[32..].copy_from_slice(&bytes);
        let expected = hex::encode(sha256_bytes(&combined));
        assert_eq!(root.0, expected);
    }
}
