//! Knowledge Attestation — cryptographic proof of document ingestion
//!
//! Provides blockchain-style chained records proving that an AI agent
//! has ingested specific documents, with Merkle tree proofs for
//! selective verification.
//!
//! # Example
//!
//! ```ignore
//! use engram::attestation::{AttestationChain, AttestationFilter, MerkleTree};
//! use engram::Storage;
//!
//! let storage = Storage::open_in_memory()?;
//! let chain = AttestationChain::new(storage);
//!
//! let record = chain.log_document(b"content", "doc.txt", Some("agent-1"), &[42], None)?;
//! let tree = MerkleTree::build(&[record]);
//! let proof = tree.generate_proof(0).unwrap();
//! assert!(MerkleTree::verify_proof(&proof));
//! ```

pub mod chain;
pub mod export;
pub mod merkle;
pub mod types;

pub use chain::AttestationChain;
pub use export::{export_csv, export_json, export_merkle_proof};
pub use merkle::MerkleTree;
pub use types::{AttestationFilter, AttestationRecord, ChainStatus, MerkleProof};
