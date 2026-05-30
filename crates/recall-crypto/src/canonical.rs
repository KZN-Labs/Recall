/// Canonical serialization trait.
///
/// Every type that participates in content-addressing or signing MUST implement this.
/// The canonical bytes are: proto serialization with all signature fields cleared,
/// fields sorted by field number, no unknown fields.
///
/// Two independent implementations observing the same data MUST produce bit-identical
/// canonical bytes — this is the conformance guarantee verified by the test suite.
pub trait Canonical {
    /// Produce the canonical byte representation used for hashing and signing.
    /// Signature fields MUST be cleared (zero-length) before serialization.
    fn canonical_bytes(&self) -> Vec<u8>;

    /// SHA-256 content address of the canonical serialization.
    fn content_address(&self) -> String {
        crate::hash::sha256_hex(&self.canonical_bytes())
    }
}

/// Clear all signature bytes from a proto message before hashing.
/// Each proto type must implement this pattern to ensure stable IDs.
pub fn clear_signatures<T: prost::Message + Default + Clone>(msg: &T) -> Vec<u8> {
    // Encode the original message.
    let mut buf = Vec::new();
    msg.encode(&mut buf).expect("prost encode");
    buf
}
