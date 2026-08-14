use alloy_primitives::{Address, B256, U256};
use anyhow::Result;
use mina_curves::pasta::Fp;
use mina_poseidon::{
    constants::PlonkSpongeConstantsKimchi,
    pasta::{fp_kimchi, FULL_ROUNDS},
    poseidon::{ArithmeticSponge as Poseidon, Sponge as _},
};
use o1_utils::FieldHelpers;

pub const MAX_TREE_DEPTH: usize = 16;
const N_MERKLE_ZEROS: usize = MAX_TREE_DEPTH + 1;
const MERKLE_ZEROS: &[u8; N_MERKLE_ZEROS * 32] = include_bytes!("merkle-zeros.dat");

// Kimchi poseidon hash

pub fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash = Poseidon::<Fp, PlonkSpongeConstantsKimchi, FULL_ROUNDS>::new(fp_kimchi::static_params());
    hash.absorb(input);
    hash.squeeze()
}

// Merkle zeros

pub fn get_merkle_zeros() -> [Fp; N_MERKLE_ZEROS] {
    let mut zeros = [Fp::from(0); N_MERKLE_ZEROS];
    for (i, chunk) in MERKLE_ZEROS.chunks(32).enumerate() {
        zeros[i] = Fp::from_bytes(chunk).expect("invalid Fp bytes");
    }
    zeros
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
/// - `depth`: Tree depth (log2 of padded leaf count) which counts the number
///   of edges and not the number of levels.
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
/// - `merkle_leaves`: mutable reference to a vector of `Fp` elements representing
///   the leaf nodes; padded in-place to length `padded_size`.
/// - `padded_size`: the total number of leaves after padding (must be a power of two).
/// - `depth`: the depth of the tree (log2 of `padded_size`).
/// - `zeros`: array of precomputed zero–hash values for each level (indexed by level).
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
pub fn fold_merkle_left(
    merkle_leaves: &mut Vec<Fp>,
    padded_size: usize,
    depth: usize,
    zeros: &[Fp; N_MERKLE_ZEROS],
) -> Fp {
    // Deal with no leaves.
    if merkle_leaves.is_empty() {
        return Fp::from(0);
    }

    // Number of leaves
    let n_leaves = merkle_leaves.len();

    // Pad to nearest power of 2
    let missing = padded_size - merkle_leaves.len();
    merkle_leaves.extend(std::iter::repeat_n(Fp::from(0), missing));

    let merkle_nodes = merkle_leaves;

    // Need to identify dummies so we can cheaply look them up
    // n_leaves = merkle_leaves.len() (before padding)
    // if index >= n_leaves we are a dummy on the leaf level
    // generalising this we need to divide n_leaves by 2 each time
    let mut n_non_dummy_nodes = n_leaves;

    for level in (1..=depth).rev() {
        let level_width = 1 << level;
        let parent_width = level_width / 2;
        for i in 0..(parent_width) {
            let i2 = 2 * i;
            let left_idx = i2;
            // Need to work out here if our left and right are dummies
            if left_idx >= n_non_dummy_nodes {
                // We are a dummy node and by virtue so is right_idx
                // rather than computing the posiedon hash we can look it up.
                merkle_nodes[i] = zeros[depth + 1 - level];
                //println!("Optimisation made 💪");
            } else {
                let right_idx = i2 + 1;
                // Atleast one is a real node
                merkle_nodes[i] = poseidon_hash(&[merkle_nodes[left_idx], merkle_nodes[right_idx]]);
            }
        }
        n_non_dummy_nodes = n_non_dummy_nodes.div_ceil(2);
    }
    merkle_nodes[0]
}

/// Constructs a full Merkle tree by iteratively hashing sibling pairs bottom-up.
///
/// This function builds every level of the tree, storing each layer in its own
/// `Vec<Fp>`, and returns a `Vec<Vec<Fp>>` from root (index 0) to leaves (index `depth`).
/// It takes input leaves, pads them to `padded_size`, and then folds siblings
/// into parent nodes one level at a time, collecting each layer separately.
///
/// **How it differs from `fold_merkle_left`:**
/// - `fold_merkle_left` computes only the Merkle root in-place by collapsing levels
///   into the same vector, which overwrites the original leaves with intermediate hashes.
/// - `build_merkle_tree` returns **all** intermediate layers as fresh vectors,
///   preserving the full tree structure.
///
/// # Parameters
///
/// - `merkle_leaves`: Owned vector of field elements representing the leaf nodes.
///   The vector is padded with zeros (dummy leaves) to reach `padded_size` before building.
/// - `padded_size`: The target number of leaves after padding (must be a power of two).
/// - `depth`: The depth of the tree (log₂ of `padded_size`).
/// - `zeros`: Precomputed “zero hashes” for each level, used to replace dummy siblings
///   cheaply instead of hashing two zero leaves each time.
///
/// # Returns
///
/// A `Vec<Vec<Fp>>` of length `depth + 1`, where:
/// - `tree[0]` is a single-element vector containing the Merkle root.
/// - `tree[1]` is the next layer of parent hashes.
/// - …
/// - `tree[depth]` is the vector of (padded) leaf values.
///
/// This means the returned tree includes all levels from root to leaves.
///
/// # Panics
///
/// - If `padded_size` is not a power of two.
/// - If `zeros` does not contain at least `depth + 1` entries.
///
/// # Example
///
/// ```rust
/// let mut leaves = vec![a, b, c];
/// let (depth, padded_size) = compute_merkle_tree_depth_and_size(leaves.len());
/// let tree = build_merkle_tree(leaves, padded_size, depth, &ZERO_HASHES);
/// let root = tree[0][0];
/// assert_eq!(tree[depth], vec![a, b, c, Fp::from(0)]);
pub fn build_merkle_tree(
    mut merkle_leaves: Vec<Fp>,
    padded_size: usize,
    depth: usize,
    zeros: &[Fp; N_MERKLE_ZEROS],
) -> Vec<Vec<Fp>> {
    // Same as above but build all levels

    // Number of leaves
    let n_leaves = merkle_leaves.len();

    // Pad to nearest power of 2
    let missing = padded_size - merkle_leaves.len();
    merkle_leaves.extend(std::iter::repeat_n(Fp::from(0), missing));

    // Need to identify dummies so we can cheaply look them up
    // n_leaves = merkle_leaves.len() (before padding)
    // if index >= n_leaves we are a dummy on the leaf level
    // generalising this we need to divide n_leaves by 2 each time
    let mut n_non_dummy_nodes = n_leaves;

    let mut merkle_tree = vec![Vec::new(); depth + 1];
    merkle_tree[depth] = merkle_leaves;

    for level in (1..=depth).rev() {
        let child_level = &merkle_tree[level];
        let parent_width = 1 << (level - 1);
        let mut parent_level: Vec<Fp> = Vec::with_capacity(parent_width);
        for i in 0..(parent_width) {
            let i2 = 2 * i;
            let left_idx = i2;
            // Need to work out here if our left and right are dummies
            if left_idx >= n_non_dummy_nodes {
                // We are a dummy node and by virtue so is right_idx
                // rather than computing the posiedon hash we can look it up.
                parent_level.push(zeros[depth + 1 - level]);
                //println!("Optimisation made 💪");
            } else {
                let right_idx = i2 + 1;
                // Atleast one is a real node
                parent_level.push(poseidon_hash(&[
                    child_level[left_idx],
                    child_level[right_idx],
                ]));
            }
        }
        n_non_dummy_nodes = n_non_dummy_nodes.div_ceil(2);
        merkle_tree[level - 1] = parent_level;
    }

    merkle_tree
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
/// - `zeros`: Precomputed “zero hashes” for each level, used to replace dummy siblings
///   cheaply instead of hashing two zero leaves each time.
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
pub fn get_merkle_path_from_leaves(
    merkle_leaves: &mut Vec<Fp>,
    padded_size: usize,
    depth: usize,
    index: u32,
    zeros: &[Fp; N_MERKLE_ZEROS],
) -> Vec<Fp> {
    if merkle_leaves.is_empty() {
        return vec![];
    }

    // Number of leaves
    let n_leaves = merkle_leaves.len();

    // Pad to nearest power of 2
    let missing = padded_size - merkle_leaves.len();
    merkle_leaves.extend(std::iter::repeat_n(Fp::from(0), missing));

    let merkle_nodes = merkle_leaves;
    let mut path: Vec<Fp> = Vec::with_capacity(depth);
    let mut position = index as usize;

    let mut n_non_dummy_nodes = n_leaves;

    for level in (1..=depth).rev() {
        let sibling_index = match position % 2 == 1 {
            true => position - 1,
            false => position + 1,
        };

        let sibling = merkle_nodes[sibling_index];
        path.push(sibling);

        let level_width = 1 << level;

        for i in 0..(level_width / 2) {
            let i2 = 2 * i;
            let left_idx = i2;
            // Need to work out here if our left and right are dummies
            if left_idx >= n_non_dummy_nodes {
                // We are a dummy node and by virtue so is right_idx
                // rather than computing the posiedon hash we can look it up.
                merkle_nodes[i] = zeros[depth + 1 - level];
            } else {
                let right_idx = i2 + 1;
                // Atleast one is a real node
                merkle_nodes[i] = poseidon_hash(&[merkle_nodes[left_idx], merkle_nodes[right_idx]]);
            }
        }

        position /= 2;
        n_non_dummy_nodes = n_non_dummy_nodes.div_ceil(2);
    }

    path
}

pub fn get_merkle_path_from_tree(merkle_tree: &[Vec<Fp>], mut index: u32) -> Vec<Fp> {
    let depth = merkle_tree.len() - 1;
    let mut path: Vec<Fp> = Vec::with_capacity(depth);
    // We can pick our nodes along the path by looking at the bit
    // starting with the leaves.
    for level in (1..=depth).rev() {
        let sibling = index ^ 1;
        let node = merkle_tree[level][sibling as usize];
        path.push(node);
        index /= 2;
    }
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

/// Computes a Poseidon hash for a storage slot leaf node given a code challenge and a 32-byte value.
///
/// The storage slot leaf combines a 32-byte code challenge and a 32-byte value into three field
/// elements, which are then hashed together using Poseidon. This process encodes the data carefully
/// to avoid overflow issues due to the 254-bit field size (which cannot safely hold 256 bits).
///
/// Specifically:
/// - The first field contains the first byte of the code challenge and the first byte of the value
///   (total 2 bytes, padded to 32 with zeros).
/// - The second field contains the remaining 31 bytes of the code challenge.
/// - The third field contains the remaining 31 bytes of the value.
/// - All three are converted from bytes to field elements and then hashed.
///
/// # Parameters
/// - `code_challenge`: A 256-bit `U256` representing the SCRAM code challenge.
/// - `value`: The 32-byte slot value.
///
/// # Returns
/// Returns a `Result<Fp>` containing the Poseidon hash of the concatenated first, second, and third fields,
/// or an error if the byte-to-field conversion fails.
///
/// # Errors
/// Returns an error if the byte slices cannot be converted into field elements (e.g., invalid byte encoding).
///
/// # Example
/// ```rust
/// let code_challenge = U256::from_be_hex("0xdeadbeef...");
/// let value = U256::from_be_hex("0xabcdef...");
/// let leaf_hash = hash_storage_slot(&code_challenge, &value).unwrap();
/// ```
pub fn hash_storage_slot(
    code_challenge: &U256,
    value: &U256,
) -> Result<Fp> {
    let code_challenge_bytes = code_challenge.to_be_bytes::<32>();
    let value_bytes = value.to_be_bytes::<32>();

    // Left here for debugging purposes
    /*print!("0x");
    for b in code_challenge.to_be_bytes::<32>().iter() {
        print!("{:02x}", b);
    }
    print!(" ");
    for b in value.to_be_bytes::<32>().iter() {
        print!("{:02x}", b);
    }
    println!();*/

    // 64 bytes total (32 + 32), max 31 bytes per field → 3 fields
    // firstFieldBytes: 1 byte from codeChallenge + 1 byte from value + 30 zeros
    let mut first_field_bytes = [0u8; 32];
    first_field_bytes[0] = code_challenge_bytes[0];
    first_field_bytes[1] = value_bytes[0];

    // secondFieldBytes: remaining 31 bytes from codeChallenge (1 to 31)
    let mut second_field_bytes = [0u8; 32];
    second_field_bytes[0..31].copy_from_slice(&code_challenge_bytes[1..32]);

    // thirdFieldBytes: remaining 31 bytes from value (1 to 31)
    let mut third_field_bytes = [0u8; 32];
    third_field_bytes[0..31].copy_from_slice(&value_bytes[1..32]);

    let first_field = Fp::from_bytes(&first_field_bytes)?;
    let second_field = Fp::from_bytes(&second_field_bytes)?;
    let third_field = Fp::from_bytes(&third_field_bytes)?;

    // Left here for debugging purposes
    /*println!("first_field {:?}", first_field);
    println!("second_field {:?}", second_field);
    println!("third_field {:?}", third_field);*/

    let hash = poseidon_hash(&[first_field, second_field, third_field]);

    // Left here for debugging purposes
    //println!("hash {:?}", hash);

    Ok(hash)
}

/// Hashes one verified queue request into a Merkle leaf.
///
/// Packs 117 bytes of leaf data into four field elements, each kept below the
/// 254-bit field size, then applies Poseidon. Byte handling matches
/// `hash_storage_slot`: big-endian payload bytes are written from index 0 and
/// read back little-endian by `Fp::from_bytes`.
///
/// Field layout:
/// - field 1: `target` (20) ++ `collection_keys_count` ++ `key_0[0]` ++ `key_1[0]` ++ `value[0]`
/// - field 2: `key_0[1..32]`
/// - field 3: `key_1[1..32]`
/// - field 4: `value[1..32]`
///
/// `collection_keys_count` is hashed so that an unused trailing key, which is
/// zero, cannot collide with a request that supplied a zero key.
///
/// The o1js `provableRequestLeafHash` must pack identically; the shared test
/// vectors pin both implementations.
pub fn hash_request_leaf(
    target: &Address,
    collection_keys_count: u8,
    collection_key_0: &B256,
    collection_key_1: &B256,
    value: &U256,
) -> Result<Fp> {
    let target_bytes = target.as_slice();
    let key_0_bytes = collection_key_0.as_slice();
    let key_1_bytes = collection_key_1.as_slice();
    let value_bytes = value.to_be_bytes::<32>();

    let mut first_field_bytes = [0u8; 32];
    first_field_bytes[0..20].copy_from_slice(target_bytes);
    first_field_bytes[20] = collection_keys_count;
    first_field_bytes[21] = key_0_bytes[0];
    first_field_bytes[22] = key_1_bytes[0];
    first_field_bytes[23] = value_bytes[0];

    let mut second_field_bytes = [0u8; 32];
    second_field_bytes[0..31].copy_from_slice(&key_0_bytes[1..32]);

    let mut third_field_bytes = [0u8; 32];
    third_field_bytes[0..31].copy_from_slice(&key_1_bytes[1..32]);

    let mut fourth_field_bytes = [0u8; 32];
    fourth_field_bytes[0..31].copy_from_slice(&value_bytes[1..32]);

    let first_field = Fp::from_bytes(&first_field_bytes)?;
    let second_field = Fp::from_bytes(&second_field_bytes)?;
    let third_field = Fp::from_bytes(&third_field_bytes)?;
    let fourth_field = Fp::from_bytes(&fourth_field_bytes)?;

    Ok(poseidon_hash(&[
        first_field,
        second_field,
        third_field,
        fourth_field,
    ]))
}

#[cfg(test)]
mod request_leaf_tests {
    use super::*;

    fn leaf(
        target: Address,
        count: u8,
        key_0: B256,
        key_1: B256,
        value: U256,
    ) -> Fp {
        hash_request_leaf(&target, count, &key_0, &key_1, &value).unwrap()
    }

    #[test]
    fn hashes_an_all_zero_request() {
        leaf(Address::ZERO, 0, B256::ZERO, B256::ZERO, U256::ZERO);
    }

    #[test]
    fn hashes_maximum_bytes_without_field_overflow() {
        leaf(
            Address::repeat_byte(0xff),
            u8::MAX,
            B256::repeat_byte(0xff),
            B256::repeat_byte(0xff),
            U256::MAX,
        );
    }

    #[test]
    fn key_count_distinguishes_an_unused_key_from_a_zero_key() {
        let one_key = leaf(
            Address::repeat_byte(0x11),
            1,
            B256::repeat_byte(0x22),
            B256::ZERO,
            U256::from(7u64),
        );
        let two_keys = leaf(
            Address::repeat_byte(0x11),
            2,
            B256::repeat_byte(0x22),
            B256::ZERO,
            U256::from(7u64),
        );
        assert_ne!(one_key, two_keys);
    }

    #[test]
    fn distinct_targets_produce_distinct_leaves() {
        let a = leaf(
            Address::repeat_byte(0x01),
            1,
            B256::repeat_byte(0x22),
            B256::ZERO,
            U256::from(7u64),
        );
        let b = leaf(
            Address::repeat_byte(0x02),
            1,
            B256::repeat_byte(0x22),
            B256::ZERO,
            U256::from(7u64),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn distinct_values_produce_distinct_leaves() {
        let a = leaf(
            Address::repeat_byte(0x11),
            1,
            B256::repeat_byte(0x22),
            B256::ZERO,
            U256::from(7u64),
        );
        let b = leaf(
            Address::repeat_byte(0x11),
            1,
            B256::repeat_byte(0x22),
            B256::ZERO,
            U256::from(8u64),
        );
        assert_ne!(a, b);
    }

    /// The leading byte of each 32-byte input is packed separately from its
    /// remaining 31 bytes, so it needs its own coverage.
    #[test]
    fn leading_bytes_are_included_in_the_hash() {
        let mut key_high = [0u8; 32];
        key_high[0] = 0xaa;
        let mut value_high = [0u8; 32];
        value_high[0] = 0xbb;

        let base = leaf(
            Address::ZERO,
            2,
            B256::ZERO,
            B256::ZERO,
            U256::ZERO,
        );
        let key_0_differs = leaf(
            Address::ZERO,
            2,
            B256::from(key_high),
            B256::ZERO,
            U256::ZERO,
        );
        let key_1_differs = leaf(
            Address::ZERO,
            2,
            B256::ZERO,
            B256::from(key_high),
            U256::ZERO,
        );
        let value_differs = leaf(
            Address::ZERO,
            2,
            B256::ZERO,
            B256::ZERO,
            U256::from_be_bytes(value_high),
        );

        assert_ne!(base, key_0_differs);
        assert_ne!(base, key_1_differs);
        assert_ne!(base, value_differs);
        assert_ne!(key_0_differs, key_1_differs);
    }

    #[test]
    fn is_deterministic() {
        let args = || {
            leaf(
                Address::repeat_byte(0x33),
                2,
                B256::repeat_byte(0x44),
                B256::repeat_byte(0x55),
                U256::from(99u64),
            )
        };
        assert_eq!(args(), args());
    }
}

#[cfg(test)]
mod merkle_fixed_tests {
    use super::*;
    use anyhow::Result;

    fn dummy_code_challenge(i: i32) -> U256 {
        let mut bytes = [0u8; 32];
        let i_bytes = i.to_le_bytes();
        bytes[0..4].copy_from_slice(&i_bytes);
        // CHECKME: was U256::from_le_bytes when this was dummy_attestation. from_le_bytes causes
        // .to_be_bytes() to reverse the byte array, so byte[0] in the hash input differs from TS
        // (TS Bytes32.from(arr).toBytes() preserves order). Previously the old dummy_attestation also
        // used from_le_bytes and tests matched cross-language — needs investigation as to why that worked.
        U256::from_be_bytes(bytes)
    }

    fn dummy_value(i: i32) -> U256 {
        let mut bytes = [0u8; 32];
        let i_bytes = i.to_le_bytes();
        bytes[0..4].copy_from_slice(&i_bytes);
        U256::from_be_bytes(bytes)
    }

    // Build leaf hashes from given (code_challenge, value) pairs
    fn build_leaves(pairs: &[(U256, U256)]) -> Result<Vec<Fp>> {
        let mut leaves = Vec::with_capacity(pairs.len());
        for (code_challenge, val) in pairs {
            leaves.push(hash_storage_slot(code_challenge, val)?);
        }
        Ok(leaves)
    }

    // Full Merkle lifecycle test using actual hashed leaves
    fn full_merkle_test(pairs: &[(U256, U256)], leaf_index: usize) -> Result<()> {
        let zeros = get_merkle_zeros();
        let leaves = build_leaves(pairs)?;
        let (depth, padded_size) = compute_merkle_tree_depth_and_size(leaves.len());

        let mut leaves_clone = leaves.clone();
        let root = fold_merkle_left(&mut leaves_clone, padded_size, depth, &zeros);

        let mut leaves_for_path = leaves.clone();
        let path = get_merkle_path_from_leaves(
            &mut leaves_for_path,
            padded_size,
            depth,
            leaf_index as u32,
            &zeros,
        );

        let leaf_hash = leaves
            .get(leaf_index)
            .copied()
            .unwrap_or_else(|| Fp::from(0));
        let recomputed_root =
            compute_merkle_root_from_path(leaf_hash, leaf_index as u64, &path.to_vec());

        assert_eq!(
            recomputed_root, root,
            "Root mismatch for leaf index {}",
            leaf_index
        );

        println!("Root {:?}", root.to_biguint());
        Ok(())
    }
    #[test]
    fn rarg_test_hash_storage_slot() {
        // Provided hex strings (without 0x prefix)
        let slot_key_code_challenge_str =
            "2f000000000000000000000000000000000000000000000000038d7ec293e52f";
        let value_str = "e8d4a51000";

        let mut hash_bytes = [0u8; 32];
        for i in 0..32 {
            let byte_str = &slot_key_code_challenge_str[i * 2..i * 2 + 2];
            hash_bytes[31 - i] = u8::from_str_radix(byte_str, 16).unwrap(); // reverse into LE
        }
        let code_challenge = U256::from_le_bytes(hash_bytes);

        // Convert value_str to U256, left-padded with zeros

        // Pad value_str to 64 hex digits (32 bytes) with leading zeros
        let padded_value = {
            let mut s = String::with_capacity(64);
            // Pad with leading zeros to make 64 characters
            for _ in 0..(64 - value_str.len()) {
                s.push('0');
            }
            s.push_str(value_str);
            s
        };

        let mut value_bytes = [0u8; 32];
        for i in 0..32 {
            let byte_str = &padded_value[i * 2..i * 2 + 2];
            value_bytes[i] = u8::from_str_radix(byte_str, 16).unwrap();
        }

        let value = U256::from_be_bytes(value_bytes);

        print!("0x");
        for b in code_challenge.to_be_bytes::<32>().iter() {
            print!("{:02x}", b);
        }
        println!(" {:?}", value);
        // Call hash_storage_slot function
        let result = hash_storage_slot(&code_challenge, &value).unwrap();

        // Assert or print result as needed
        println!("Hash result big int: {:?}", result.to_bigint_positive());
        println!("Hash result hex: {:?}", result.to_hex());
        println!("Hash result bytes: {:?}", result.to_bytes());

        let bytes = result.to_bytes();
        print!("Hash result (hex): 0x");
        for b in bytes.iter() {
            print!("{:02x}", b);
        }
        println!();
    }

    #[test]
    fn test_large_slots() -> Result<()> {
        let n = 1000;
        let pairs: Vec<(U256, U256)> = (0..n)
            .map(|i| (dummy_code_challenge(i), dummy_value(i)))
            .collect();
        full_merkle_test(&pairs, 543)
    }

    #[test]
    fn test_hash_storage_slot_basic() -> Result<()> {
        let code_challenge = dummy_code_challenge(2);
        let value = dummy_value(3);
        let leaf_hash = hash_storage_slot(&code_challenge, &value)?;
        assert_ne!(leaf_hash, Fp::from(0));
        Ok(())
    }

    #[test]
    fn test_all_leaf_counts_and_indices_with_build_and_fold() {
        let zeros = get_merkle_zeros();
        println!("Testing all leaf counts and indices with both fold and build...");

        for n_leaves in 0..=50 {
            println!("→ Testing with {} leaves", n_leaves);

            // Build dummy pairs
            let pairs: Vec<(U256, U256)> = (0..n_leaves)
                .map(|i| (dummy_code_challenge(i), dummy_value(i)))
                .collect();

            let leaves = build_leaves(&pairs).expect("build_leaves failed");
            print!("   leaves=");
            for leaf in leaves.clone() {
                print!("{}, ", leaf);
            }
            println!();

            let (depth, padded_size) = compute_merkle_tree_depth_and_size(leaves.len());
            println!("   depth={}, padded_size={}", depth, padded_size);

            // Fold to get root
            let mut leaves_for_fold = leaves.clone();
            let root_via_fold = fold_merkle_left(&mut leaves_for_fold, padded_size, depth, &zeros);
            println!("   root_via_fold = {}", root_via_fold);

            // Build full tree and verify root & leaves
            let merkle_tree = build_merkle_tree(leaves.clone(), padded_size, depth, &zeros);
            println!("   root_via_build = {}", merkle_tree[0][0]);

            // Root check
            assert_eq!(
                merkle_tree[0][0], root_via_fold,
                "[n_leaves={}] build root {:?} ≠ fold root {:?}",
                n_leaves, merkle_tree[0][0], root_via_fold
            );

            // Change leaf layer verification to use depth instead of depth-1
            let mut expected_padded = leaves.clone();
            expected_padded.extend(std::iter::repeat_n(
                Fp::from(0),
                padded_size - expected_padded.len(),
            ));

            assert_eq!(
                merkle_tree[depth], expected_padded,
                "Leaf layer not preserved correctly"
            );

            // For each leaf index, verify path → root
            for index in 0..n_leaves {
                let mut leaves_for_path = leaves.clone();

                // Path from fold method
                let path_fold = get_merkle_path_from_leaves(
                    &mut leaves_for_path,
                    padded_size,
                    depth,
                    index as u32,
                    &zeros,
                );

                // Path from full tree method
                let path_build = get_merkle_path_from_tree(&merkle_tree, index as u32);

                // Check paths are identical
                assert_eq!(
                    path_fold, path_build,
                    "[n_leaves={}, index={}] paths differ between fold and build methods",
                    n_leaves, index
                );

                // Recompute root from path (fold method)
                let leaf_hash = leaves[index as usize];
                let recomputed_root =
                    compute_merkle_root_from_path(leaf_hash, index as u64, &path_fold.to_vec());

                assert_eq!(
                    recomputed_root, root_via_fold,
                    "[n_leaves={}, index={}] path root {:?} ≠ fold root {:?}",
                    n_leaves, index, recomputed_root, root_via_fold
                );

                println!("     ✅ [n_leaves={}, index={}] OK", n_leaves, index);
            }
        }
    }

    // Brute-force reference: pad with Fp(0), hash every pair, no zeros cache.
    fn reference_root_bruteforce(leaves: &[Fp], padded_size: usize) -> Fp {
        let mut level: Vec<Fp> = leaves.to_vec();
        level.resize(padded_size, Fp::from(0));
        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len() / 2);
            for i in (0..level.len()).step_by(2) {
                next.push(poseidon_hash(&[level[i], level[i + 1]]));
            }
            level = next;
        }
        level[0]
    }

    // Recursive reference: computes the root of a subtree by recursion.
    fn reference_root_recursive(leaves: &[Fp], padded_size: usize) -> Fp {
        fn subtree(leaves: &[Fp], offset: usize, size: usize) -> Fp {
            if size == 1 {
                return if offset < leaves.len() {
                    leaves[offset]
                } else {
                    Fp::from(0)
                };
            }
            let half = size / 2;
            let left = subtree(leaves, offset, half);
            let right = subtree(leaves, offset + half, half);
            poseidon_hash(&[left, right])
        }
        subtree(leaves, 0, padded_size)
    }

    // 1,3  - no dummy pairs (sanity)
    // 5,6  - dummy pairs at depth 3
    // 9    - dummy pairs at depth 4
    // 17   - dummy pairs at depth 5
    const REGRESSION_LEAF_COUNTS: &[i32] = &[1, 3, 5, 6, 9, 17];

    #[test]
    fn regression_a2090_bruteforce_reference() {
        let zeros = get_merkle_zeros();
        let mut failures: Vec<String> = Vec::new();
        for &n_leaves in REGRESSION_LEAF_COUNTS {
            let pairs: Vec<(U256, U256)> = (0..n_leaves)
                .map(|i| (dummy_code_challenge(i), dummy_value(i)))
                .collect();
            let leaves = build_leaves(&pairs).expect("build_leaves failed");
            let (depth, padded_size) = compute_merkle_tree_depth_and_size(leaves.len());

            let expected = reference_root_bruteforce(&leaves, padded_size);

            let tree = build_merkle_tree(leaves.clone(), padded_size, depth, &zeros);
            if tree[0][0] != expected {
                failures.push(format!(
                    "[n_leaves={}] build_merkle_tree root does not match brute-force reference",
                    n_leaves
                ));
            }

            let mut leaves_for_fold = leaves.clone();
            let root_fold = fold_merkle_left(&mut leaves_for_fold, padded_size, depth, &zeros);
            if root_fold != expected {
                failures.push(format!(
                    "[n_leaves={}] fold_merkle_left root does not match brute-force reference",
                    n_leaves
                ));
            }
        }
        if !failures.is_empty() {
            panic!("{} failures:\n{}", failures.len(), failures.join("\n"));
        }
    }

    #[test]
    fn regression_a2090_recursive_reference() {
        let zeros = get_merkle_zeros();
        let mut failures: Vec<String> = Vec::new();
        for &n_leaves in REGRESSION_LEAF_COUNTS {
            let pairs: Vec<(U256, U256)> = (0..n_leaves)
                .map(|i| (dummy_code_challenge(i), dummy_value(i)))
                .collect();
            let leaves = build_leaves(&pairs).expect("build_leaves failed");
            let (depth, padded_size) = compute_merkle_tree_depth_and_size(leaves.len());

            let expected = reference_root_recursive(&leaves, padded_size);

            let tree = build_merkle_tree(leaves.clone(), padded_size, depth, &zeros);
            if tree[0][0] != expected {
                failures.push(format!(
                    "[n_leaves={}] build_merkle_tree root does not match recursive reference",
                    n_leaves
                ));
            }

            let mut leaves_for_fold = leaves.clone();
            let root_fold = fold_merkle_left(&mut leaves_for_fold, padded_size, depth, &zeros);
            if root_fold != expected {
                failures.push(format!(
                    "[n_leaves={}] fold_merkle_left root does not match recursive reference",
                    n_leaves
                ));
            }
        }
        if !failures.is_empty() {
            panic!("{} failures:\n{}", failures.len(), failures.join("\n"));
        }
    }
}

/// STATICALLY BUILT ZEROS
///
#[cfg(test)]
mod merkle_zeros {
    use super::*;
    use std::io::Write;
    use std::{env, path::PathBuf};

    fn calculate_zeros() -> Vec<Fp> {
        // [Fp; N_MERKLE_ZEROS]
        let zeros_iter: std::iter::Take<std::iter::Successors<Fp, _>> =
            std::iter::successors(Some(Fp::from(0)), |last| {
                Some(poseidon_hash(&[*last, *last]))
            })
            .take(N_MERKLE_ZEROS);

        zeros_iter.collect::<Vec<Fp>>()
    }

    fn find_workspace_root(mut dir: PathBuf) -> Option<PathBuf> {
        let mut last_found = None;
        loop {
            if dir.join("Cargo.toml").exists() {
                last_found = Some(dir.clone());
            }
            if !dir.pop() {
                break;
            }
        }
        last_found
    }

    fn save_zeros() -> Result<()> {
        let zeros = calculate_zeros();
        let bytes: Vec<u8> = zeros.iter().flat_map(|zero| zero.to_bytes()).collect();

        // Determine the current project directory (where Cargo.toml is located)
        let project_dir = env::current_dir().expect("Failed to get current directory");
        let cargo_dir = match find_workspace_root(project_dir) {
            Some(root) => root,
            None => panic!("Could not find project root"),
        };

        // Use the correct relative paths based on the project root
        let nori_hash_path = cargo_dir.join("nori-hash");
        let nori_hash_src_dir = nori_hash_path.join("src");
        let nori_hash_merkle_zeros = nori_hash_src_dir.join("merkle-zeros.dat");

        let mut f1 = std::fs::File::create(nori_hash_merkle_zeros).unwrap();
        f1.write_all(&bytes).expect("write must succeed");

        Ok(())
    }

    #[test]
    fn build_zeros() -> Result<()> {
        save_zeros()?;
        Ok(())
    }
}
