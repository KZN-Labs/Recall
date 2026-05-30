use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid public key bytes")]
    InvalidPublicKey,
    #[error("invalid signature bytes")]
    InvalidSignature,
    #[error("signature verification failed")]
    VerificationFailed,
}

/// Ed25519 signing keypair.
pub struct RecallKeypair {
    signing_key: SigningKey,
}

impl RecallKeypair {
    /// Generate a fresh keypair using the OS random source.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// Restore from raw 32-byte secret scalar.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            signing_key: SigningKey::from_bytes(bytes),
        }
    }

    pub fn public_key(&self) -> RecallPublicKey {
        RecallPublicKey(self.signing_key.verifying_key())
    }

    /// Sign arbitrary data. Returns 64-byte raw signature.
    pub fn sign(&self, msg: &[u8]) -> RecallSignature {
        RecallSignature(self.signing_key.sign(msg))
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }
}

#[derive(Clone, Debug)]
pub struct RecallPublicKey(pub VerifyingKey);

impl RecallPublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let arr: &[u8; 32] = bytes.try_into().map_err(|_| CryptoError::InvalidPublicKey)?;
        Ok(Self(
            VerifyingKey::from_bytes(arr).map_err(|_| CryptoError::InvalidPublicKey)?,
        ))
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn verify(&self, msg: &[u8], sig: &RecallSignature) -> Result<(), CryptoError> {
        self.0
            .verify(msg, &sig.0)
            .map_err(|_| CryptoError::VerificationFailed)
    }
}

#[derive(Clone, Debug)]
pub struct RecallSignature(pub ed25519_dalek::Signature);

impl RecallSignature {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let arr: &[u8; 64] = bytes.try_into().map_err(|_| CryptoError::InvalidSignature)?;
        Ok(Self(ed25519_dalek::Signature::from_bytes(arr)))
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        self.0.to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let kp = RecallKeypair::generate();
        let msg = b"recall memory entry content-address";
        let sig = kp.sign(msg);
        kp.public_key().verify(msg, &sig).expect("verification failed");
    }

    #[test]
    fn tampered_message_fails_verification() {
        let kp = RecallKeypair::generate();
        let sig = kp.sign(b"original");
        assert!(kp.public_key().verify(b"tampered", &sig).is_err());
    }
}
