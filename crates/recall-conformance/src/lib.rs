/// Conformance test suite for RECALL canonical serialization.
///
/// The core invariant: two independent implementations observing the same action
/// MUST produce bit-identical receipt IDs (SHA-256 of canonical proto bytes,
/// signatures cleared). This suite verifies that invariant in Rust and
/// provides the test vectors that Python and Go must also pass.
///
/// See spec/vectors/receipt-canonical-v1.json for the canonical vectors.

pub mod receipt_vectors;
pub mod passport_vectors;

pub use receipt_vectors::run_receipt_conformance;
pub use passport_vectors::run_passport_conformance;
