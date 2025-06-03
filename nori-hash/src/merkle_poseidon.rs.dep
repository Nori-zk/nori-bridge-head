use alloy_primitives::{Address, FixedBytes};
use anyhow::Result;
use kimchi::{
    mina_curves::pasta::Fp,
    mina_poseidon::{
        constants::PlonkSpongeConstantsKimchi,
        pasta::fp_kimchi,
        poseidon::{ArithmeticSponge as Poseidon, Sponge as _},
    },
    o1_utils::FieldHelpers,
};

// Kimchi poseidon hash

pub fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash = Poseidon::<Fp, PlonkSpongeConstantsKimchi>::new(fp_kimchi::static_params());
    hash.absorb(input);
    hash.squeeze()
}

/// Computes the Merkle tree depth and padded size for a given number of leaves.
///
/// For a valid Merkle tree structure:
/// - Returns depth = 0 and padded_size = 1 when n_leaves == 0
/// - Returns depth = 0 and padded_size = 1 when n_leaves == 1
/// - For n_leaves > 1, computes the next power-of-two padded size
///   and corresponding tree depth
///
/// # Parameters
/// - `n_leaves`: Number of leaf nodes in the Merkle tree
///
/// # Returns
/// Tuple `(depth, padded_size)` where:
/// - `depth`: Tree depth (log2 of padded leaf count)
/// - `padded_size`: Next power-of-two size for padding
///
/// # Examples
/// ```
/// assert_eq!(compute_merkle_tree_depth_and_size(0), (0, 1));
/// assert_eq!(compute_merkle_tree_depth_and_size(1), (0, 1));
/// assert_eq!(compute_merkle_tree_depth_and_size(2), (1, 2));
/// assert_eq!(compute_merkle_tree_depth_and_size(3), (2, 4));
/// assert_eq!(compute_merkle_tree_depth_and_size(4), (2, 4));
/// assert_eq!(compute_merkle_tree_depth_and_size(5), (3, 8));
/// ```
pub fn compute_merkle_tree_depth_and_size(n_leaves: usize) -> (usize, usize) {
    match n_leaves {
        0 | 1 => (0, 1),
        _ => {
            let padded_size = n_leaves.next_power_of_two();
            let depth = padded_size.trailing_zeros() as usize;
            (depth, padded_size)
        }
    }
}

/// Folds a Merkle tree in-place by iteratively hashing sibling pairs leftward.
///
/// The function performs a leftward folding operation at each tree level:
///
/// 1. At each level, sibling pairs are hashed together
/// 2. The resulting parent nodes are stored in the left half of the current level's data
/// 3. The tree effectively folds in half with each iteration
/// 4. The computation collapses toward the leftmost position (index 0)
///
/// After completion, the first element of `merkle_leaves` contains the Merkle root.
/// The contents of the rest of the vector are intermediate hashes and should not
/// be relied upon.
///
/// # Parameters
///
/// - `merkle_leaves`: Mutable reference to a vector of field elements representing
///   the leaves, padded to a power-of-two length.
///
/// - `depth`: The depth of the tree (log2 of `merkle_leaves.len()`).
///
/// # Returns
///
/// The computed Merkle root as an `Fp` element.
///
/// # Example
///
/// ```rust
/// // Assumes merkle_leaves is populated.
/// let root = fold_merkle_left(&mut merkle_leaves, depth);
/// ```
pub fn fold_merkle_left(merkle_leaves: &mut Vec<Fp>, padded_size: usize, depth: usize) -> Fp {
    // Deal with no leaves.
    if merkle_leaves.is_empty() {
        return Fp::from(0);
    }

    // Pad to nearest power of 2
    let missing = padded_size - merkle_leaves.len();
    merkle_leaves.extend(std::iter::repeat(Fp::from(0)).take(missing));

    let merkle_nodes = merkle_leaves;

    for level in (1..=depth).rev() {
        let level_width = 1 << level;
        for i in 0..(level_width / 2) {
            let left = merkle_nodes[2 * i];
            let right = merkle_nodes[2 * i + 1];
            merkle_nodes[i] = poseidon_hash(&[left, right]);
        }
    }
    merkle_nodes[0]
}

/// Computes the Merkle authentication path for a given leaf index, mutating the leaf vector in-place.
///
/// This function destructively folds a Merkle tree from a vector of leaf nodes while collecting
/// the Merkle authentication path (i.e., the list of sibling nodes) for a specific leaf at `index`.
///
/// The vector `merkle_leaves` is treated as scratch space and will be **corrupted** during execution.
/// All intermediate parent nodes are written in-place by overwriting the start of the vector at
/// each level. No heap allocations are made beyond the returned path vector.
///
/// ## Path Semantics
/// - The returned path contains the **sibling node** at each level of the Merkle tree,
///   starting from the leaf level up to (but not including) the root.
/// - The path is ordered bottom-up: index 0 is the sibling at the leaf level, index `depth - 1` is at the root level.
/// - The caller must ensure that `index` refers to a valid leaf index within the padded tree.
///
/// ## Parameters
/// - `merkle_leaves`: A mutable vector of `Fp` elements representing the leaf nodes of the tree.
///   This vector will be padded with zeroes (if needed) and overwritten during processing.
/// - `padded_size`: The expected number of leaves after padding (must be a power of two).
/// - `depth`: The depth of the tree (log₂ of `padded_size`; zero for trees with ≤1 leaf).
/// - `index`: The index of the leaf for which the Merkle path is to be computed.
///
/// ## Returns
/// A vector of `Fp` elements, each representing a sibling node in the Merkle path.
/// The path contains exactly `depth` elements.
///
/// ## Panics
/// Panics if `index >= padded_size`.
///
/// ## Example
/// ```rust
/// let (depth, padded_size) = compute_merkle_tree_depth_and_size(leaves.len());
/// let mut leaves = original_leaf_values.clone();
/// let path = get_merkle_path(&mut leaves, padded_size, depth, 2);
/// ```
pub fn get_merkle_path(
    merkle_leaves: &mut Vec<Fp>,
    padded_size: usize,
    depth: usize,
    index: u32,
) -> Vec<Fp> {
    if merkle_leaves.is_empty() {
        return vec![];
    }

    // Pad to nearest power of 2
    let missing = padded_size - merkle_leaves.len();
    merkle_leaves.extend(std::iter::repeat(Fp::from(0)).take(missing));

    let merkle_nodes = merkle_leaves;
    let mut path: Vec<Fp> = Vec::with_capacity(depth);
    let mut position = index as usize;

    for level in (1..=depth).rev() {
        let sibling_index = match position % 2 == 1 {
            true => position - 1,
            false => position + 1,
        };

        let sibling = merkle_nodes[sibling_index];
        path.push(sibling);

        let level_width = 1 << level;

        for i in 0..(level_width / 2) {
            let left = merkle_nodes[2 * i];
            let right = merkle_nodes[2 * i + 1];
            merkle_nodes[i] = poseidon_hash(&[left, right]);
        }

        position /= 2;
    }

    // I fear we may have skipped a node
    path
}

/// Recomputes the Merkle root from a leaf hash, its index, and its authentication path.
///
/// This function traverses the Merkle authentication path bottom-up, using the provided
/// leaf hash and sibling hashes to reconstruct the Merkle root. At each level, it combines
/// the current hash with its sibling according to the corresponding bit of the index,
/// then applies the Poseidon hash.
///
/// ## Path Semantics
/// - The `path` slice contains sibling hashes starting from the leaf level up to the root level.
/// - The `leaf_hash` is the already hashed leaf value.
/// - The `index` is the 0-based leaf index in the padded Merkle tree.
///
/// ## Parameters
/// - `leaf_hash`: Poseidon hash of the leaf value.
/// - `index`: The leaf index within the tree.
/// - `path`: Slice of sibling hashes for each level of the tree.
///
/// ## Returns
/// The Merkle root as an `Fp` element.
///
/// ## Panics
/// Panics if `path.len()` is larger than 64 bits (index must fit in u64).
///
/// ## Example
/// ```rust
/// let leaf = poseidon_hash(&[Fp::from(42)]);
/// let root = compute_merkle_root_from_path(leaf, 2, &path);
/// ```
pub fn compute_merkle_root_from_path(leaf_hash: Fp, index: u64, path: &[Fp]) -> Fp {
    let mut hash = leaf_hash;

    for (level, sibling) in path.iter().enumerate() {
        // Extract bit at current level to decide ordering
        let bit = (index >> level) & 1;

        let (left, right) = if bit == 1 {
            (*sibling, hash)
        } else {
            (hash, *sibling)
        };

        hash = poseidon_hash(&[left, right]);
    }

    hash
}

/// Computes a Poseidon hash for a storage slot leaf node given a contract address and a 32-byte value.
///
/// The storage slot leaf combines the 20-byte contract address and a 32-byte value into two field elements,
/// which are then hashed together using Poseidon. This process encodes the data carefully to avoid
/// overflow issues due to the 254-bit field size (which cannot safely hold 256 bits).
///
/// Specifically:
/// - The first field contains the full 20-byte address plus the first byte of the value (total 21 bytes).
/// - The second field contains the remaining 31 bytes of the value.
/// - Both are converted from bytes to field elements and then hashed.
///
/// # Parameters
/// - `address`: Reference to a 20-byte contract `Address`.
/// - `value`: A 32-byte fixed-length array representing the slot value.
///
/// # Returns
/// Returns a `Result<Fp>` containing the Poseidon hash of the concatenated address and value fields,
/// or an error if the byte-to-field conversion fails.
///
/// # Errors
/// Returns an error if the byte slices cannot be converted into field elements (e.g., invalid byte encoding).
///
/// # Example
/// ```rust
/// let address = Address::from_hex("0x1234567890abcdef1234567890abcdef12345678").unwrap();
/// let value = FixedBytes::from_hex("0xabcdef...").unwrap();
/// let leaf_hash = hash_storage_slot_leaf(&address, value).unwrap();
/// ```
pub fn hash_storage_slot(address: &Address, value: &FixedBytes<32>) -> Result<Fp> {
    // First encode the bytes into fields
    // We have 20 bytes on the Address and 32 on value
    // We cannot have 32 Bytes on value because we fields are 254bits and 8*32 is 256 which would be an overflow
    // So we need a max number of bytes 31 248.
    // We can move one bit from the value to the address encode 2 fields and hash to convert to one field
    let address_slice = address.as_slice();
    let value_slice = value.as_slice();
    let mut first_field_bytes = [0u8; 32];
    first_field_bytes[0..20].copy_from_slice(&address_slice[0..20]);
    first_field_bytes[20] = value_slice[0];

    let mut second_field_bytes = [0u8; 32];
    second_field_bytes[..31].copy_from_slice(&value_slice[1..32]);
    let first_field = Fp::from_bytes(&first_field_bytes)?;
    let second_field = Fp::from_bytes(&second_field_bytes)?;
    let fields = [first_field, second_field];
    Ok(poseidon_hash(&fields))
}

#[cfg(test)]
mod merkle_tests {
    use super::*;
    use alloy_primitives::{Address, FixedBytes};
    use anyhow::Result;

    // Helper: Generate dummy Address with bytes all set to given value
    fn dummy_address(byte: u8) -> Address {
        let bytes = [byte; 20];
        Address::from_slice(&bytes)
    }

    // Helper: Generate dummy FixedBytes<32> with bytes all set to given value
    fn dummy_value(byte: u8) -> FixedBytes<32> {
        FixedBytes::<32>::new([byte; 32])
    }

    // Build leaf hashes from given (address, value) pairs
    fn build_leaves(pairs: &[(Address, FixedBytes<32>)]) -> Result<Vec<Fp>> {
        let mut leaves = Vec::with_capacity(pairs.len());
        for (addr, val) in pairs {
            leaves.push(hash_storage_slot(addr, val)?);
        }
        Ok(leaves)
    }

    // Full Merkle lifecycle test using your actual hashed leaves
    fn full_merkle_test(pairs: &[(Address, FixedBytes<32>)], leaf_index: usize) -> Result<()> {
        let leaves = build_leaves(pairs)?;
        let (depth, padded_size) = compute_merkle_tree_depth_and_size(leaves.len());

        let mut leaves_clone = leaves.clone();
        let root = fold_merkle_left(&mut leaves_clone, padded_size, depth);

        let mut leaves_for_path = leaves.clone();
        let path = get_merkle_path(&mut leaves_for_path, padded_size, depth, leaf_index as u32);

        let leaf_hash = leaves.get(leaf_index).copied().unwrap_or_else(|| Fp::from(0));
        let recomputed_root = compute_merkle_root_from_path(leaf_hash, leaf_index as u64, &path);

        assert_eq!(
            recomputed_root, root,
            "Root mismatch for leaf index {}",
            leaf_index
        );
        Ok(())
    }

    #[test]
    fn test_zero_slots() -> Result<()> {
        let pairs = vec![];
        // No leaves, leaf_index 0 is arbitrary but should not panic
        full_merkle_test(&pairs, 0)
    }

    #[test]
    fn test_one_slot() -> Result<()> {
        let pairs = vec![(dummy_address(1), dummy_value(1))];
        full_merkle_test(&pairs, 0)
    }

    #[test]
    fn test_two_slots() -> Result<()> {
        let pairs = vec![
            (dummy_address(1), dummy_value(1)),
            (dummy_address(2), dummy_value(2)),
        ];
        full_merkle_test(&pairs, 1)
    }

    #[test]
    fn test_three_slots() -> Result<()> {
        let pairs = vec![
            (dummy_address(1), dummy_value(1)),
            (dummy_address(2), dummy_value(2)),
            (dummy_address(3), dummy_value(3)),
        ];
        full_merkle_test(&pairs, 2)
    }

    #[test]
    fn test_four_slots() -> Result<()> {
        let pairs = vec![
            (dummy_address(1), dummy_value(1)),
            (dummy_address(2), dummy_value(2)),
            (dummy_address(3), dummy_value(3)),
            (dummy_address(4), dummy_value(4)),
        ];
        full_merkle_test(&pairs, 3)
    }

    #[test]
    fn test_large_slots() -> Result<()> {
        let n = 1000;
        let pairs: Vec<(Address, FixedBytes<32>)> = (0..n)
            .map(|i| (dummy_address(i as u8), dummy_value(i as u8)))
            .collect();
        full_merkle_test(&pairs, 543)
    }

    #[test]
    fn test_hash_storage_slot_basic() -> Result<()> {
        let address = dummy_address(1);
        let value = dummy_value(2);
        let leaf_hash = hash_storage_slot(&address, &value)?;
        assert_ne!(leaf_hash, Fp::from(0));
        Ok(())
    }
}
