//! Merkle tree for attestation proofs
//!
//! Builds a binary Merkle tree over a set of attestation records.
//! Leaves are the `record_hash` values; parent nodes are the SHA-256 hash
//! of the concatenation of their left and right children.
//! When the number of leaves is odd the last leaf is duplicated.

use sha2::{Digest, Sha256};

use super::types::{AttestationRecord, MerkleProof};

/// Binary Merkle tree built from attestation record hashes
pub struct MerkleTree {
    /// Level 0 = raw leaves (record_hash values), last level = [root]
    levels: Vec<Vec<String>>,
}

impl MerkleTree {
    /// Build a Merkle tree from a slice of attestation records.
    ///
    /// Returns an empty tree (no levels) when `records` is empty.
    pub fn build(records: &[AttestationRecord]) -> Self {
        if records.is_empty() {
            return Self { levels: Vec::new() };
        }

        // Collect leaf hashes
        let leaves: Vec<String> = records
            .iter()
            .map(|r| r.record_hash.clone())
            .collect();

        let mut levels: Vec<Vec<String>> = Vec::new();
        levels.push(leaves);

        // Build levels bottom-up until we reach the root
        while levels.last().map(|l| l.len()).unwrap_or(0) > 1 {
            let current = levels.last().unwrap();
            let mut next: Vec<String> = Vec::new();

            let mut i = 0;
            while i < current.len() {
                let left = &current[i];
                // Duplicate last leaf if count is odd
                let right = if i + 1 < current.len() {
                    &current[i + 1]
                } else {
                    &current[i]
                };
                next.push(hash_pair(left, right));
                i += 2;
            }
            levels.push(next);
        }

        Self { levels }
    }

    /// Return the root hash of the tree, or `None` if the tree is empty.
    pub fn root(&self) -> Option<&str> {
        self.levels.last()?.first().map(String::as_str)
    }

    /// Generate a Merkle proof for the leaf at `leaf_index`.
    ///
    /// Returns `None` if the tree is empty or the index is out of bounds.
    pub fn generate_proof(&self, leaf_index: usize) -> Option<MerkleProof> {
        if self.levels.is_empty() {
            return None;
        }

        let leaves = &self.levels[0];
        if leaf_index >= leaves.len() {
            return None;
        }

        let leaf_hash = leaves[leaf_index].clone();
        let total_leaves = leaves.len();
        let root_hash = self.root()?.to_string();

        let mut proof_hashes: Vec<(String, bool)> = Vec::new();
        let mut current_index = leaf_index;

        for level in &self.levels[..self.levels.len().saturating_sub(1)] {
            // Determine sibling index and position
            let (sibling_index, is_right_sibling) = if current_index.is_multiple_of(2) {
                // We are the left child; sibling is to the right
                let sibling = if current_index + 1 < level.len() {
                    current_index + 1
                } else {
                    current_index // duplicated
                };
                (sibling, true)
            } else {
                // We are the right child; sibling is to the left
                (current_index - 1, false)
            };

            proof_hashes.push((level[sibling_index].clone(), is_right_sibling));
            current_index /= 2;
        }

        Some(MerkleProof {
            leaf_hash,
            leaf_index,
            proof_hashes,
            root_hash,
            total_leaves,
        })
    }

    /// Verify a Merkle proof.
    ///
    /// Recomputes the root from the leaf hash and the proof hashes, then
    /// compares against `proof.root_hash`.
    pub fn verify_proof(proof: &MerkleProof) -> bool {
        let mut current = proof.leaf_hash.clone();

        for (sibling_hash, is_right_sibling) in &proof.proof_hashes {
            current = if *is_right_sibling {
                // sibling is to the right → we are the left child
                hash_pair(&current, sibling_hash)
            } else {
                // sibling is to the left → we are the right child
                hash_pair(sibling_hash, &current)
            };
        }

        current == proof.root_hash
    }
}

/// Compute SHA-256 of the concatenated raw bytes of two hash strings.
fn hash_pair(left: &str, right: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(left.as_bytes());
    hasher.update(right.as_bytes());
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::chain::AttestationChain;
    use crate::storage::Storage;
    use chrono::Utc;

    fn make_record(hash: &str, name: &str) -> AttestationRecord {
        AttestationRecord {
            id: None,
            document_hash: format!("sha256:{hash}"),
            document_name: name.to_string(),
            document_size: 0,
            ingested_at: Utc::now(),
            agent_id: None,
            memory_ids: vec![],
            previous_hash: "genesis".to_string(),
            record_hash: format!("sha256:{hash}-record"),
            signature: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            created_at: None,
        }
    }

    #[test]
    fn test_empty_tree() {
        let tree = MerkleTree::build(&[]);
        assert!(tree.root().is_none());
        assert!(tree.generate_proof(0).is_none());
    }

    #[test]
    fn test_single_leaf() {
        let rec = make_record("aabbcc", "a.txt");
        let tree = MerkleTree::build(&[rec.clone()]);
        assert_eq!(tree.root(), Some(rec.record_hash.as_str()));

        let proof = tree.generate_proof(0).unwrap();
        assert_eq!(proof.leaf_hash, rec.record_hash);
        assert_eq!(proof.total_leaves, 1);
        assert!(MerkleTree::verify_proof(&proof));
    }

    #[test]
    fn test_two_leaves() {
        let r1 = make_record("aa", "a.txt");
        let r2 = make_record("bb", "b.txt");
        let tree = MerkleTree::build(&[r1.clone(), r2.clone()]);

        let expected_root = hash_pair(&r1.record_hash, &r2.record_hash);
        assert_eq!(tree.root(), Some(expected_root.as_str()));

        let proof0 = tree.generate_proof(0).unwrap();
        assert!(MerkleTree::verify_proof(&proof0));

        let proof1 = tree.generate_proof(1).unwrap();
        assert!(MerkleTree::verify_proof(&proof1));
    }

    #[test]
    fn test_three_leaves_odd() {
        let records: Vec<_> = ["aa", "bb", "cc"]
            .iter()
            .enumerate()
            .map(|(i, h)| make_record(h, &format!("{i}.txt")))
            .collect();

        let tree = MerkleTree::build(&records);
        assert!(tree.root().is_some());

        for i in 0..3 {
            let proof = tree.generate_proof(i).unwrap();
            assert!(MerkleTree::verify_proof(&proof), "proof {i} failed");
        }
    }

    #[test]
    fn test_proof_from_real_chain() {
        let storage = Storage::open_in_memory().unwrap();
        let chain = AttestationChain::new(storage);

        let r1 = chain.log_document(b"doc1", "d1.txt", None, &[], None).unwrap();
        let r2 = chain.log_document(b"doc2", "d2.txt", None, &[], None).unwrap();
        let r3 = chain.log_document(b"doc3", "d3.txt", None, &[], None).unwrap();

        let tree = MerkleTree::build(&[r1, r2, r3]);
        for i in 0..3 {
            let proof = tree.generate_proof(i).unwrap();
            assert!(MerkleTree::verify_proof(&proof), "real chain proof {i} failed");
        }
    }

    #[test]
    fn test_tampered_proof_fails() {
        let r1 = make_record("aa", "a.txt");
        let r2 = make_record("bb", "b.txt");
        let tree = MerkleTree::build(&[r1, r2]);

        let mut proof = tree.generate_proof(0).unwrap();
        // Tamper with the root
        proof.root_hash = "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string();
        assert!(!MerkleTree::verify_proof(&proof));
    }
}
