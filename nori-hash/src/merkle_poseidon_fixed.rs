use alloy_primitives::{Address, FixedBytes, U256};
use anyhow::Result;
use mina_curves::pasta::Fp;
use mina_poseidon::{
    constants::PlonkSpongeConstantsKimchi,
    pasta::fp_kimchi,
    poseidon::{ArithmeticSponge as Poseidon, Sponge as _},
};
use o1_utils::FieldHelpers;

pub const MAX_TREE_DEPTH: usize = 16;
const N_MERKLE_ZEROS: usize = MAX_TREE_DEPTH + 1;
const MERKLE_ZEROS: &[u8; N_MERKLE_ZEROS * 32] = include_bytes!("merkle-zeros.dat");

// Kimchi poseidon hash

pub fn poseidon_hash(input: &[Fp]) -> Fp {
    let mut hash = Poseidon::<Fp, PlonkSpongeConstantsKimchi>::new(fp_kimchi::static_params());
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
                merkle_nodes[i] = zeros[level];
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
                parent_level.push(zeros[level]);
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
                merkle_nodes[i] = zeros[level];
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
/*pub fn hash_storage_slot(address: &Address, value: &FixedBytes<32>) -> Result<Fp> {
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
}*/

/// Computes a Poseidon hash for a storage slot leaf node given a contract address,
/// an attestation hash, and a 32-byte value.
///
/// The storage slot leaf combines the 20-byte contract address, a 32-byte attestation hash,
/// and a 32-byte value into three field elements, which are then hashed together using Poseidon.
/// This process encodes the data carefully to avoid overflow issues due to the 254-bit field size
/// (which cannot safely hold 256 bits).
///
/// Specifically:
/// - The first field contains the 20-byte address, the first byte of the attestation hash,
///   and the first byte of the value (total 22 bytes).
/// - The second field contains the remaining 31 bytes of the attestation hash.
/// - The third field contains the remaining 31 bytes of the value.
/// - All three are converted from bytes to field elements and then hashed.
///
/// # Parameters
/// - `address`: Reference to a 20-byte Address key.
/// - `attestation_hash`: A 256-bit `U256` representing the attestation_hash key.
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
/// let address = Address::from_hex("0x1234567890abcdef1234567890abcdef12345678").unwrap();
/// let attestation_hash = U256::from_be_hex("0xdeadbeef...");
/// let value = FixedBytes::from_hex("0xabcdef...").unwrap();
/// let leaf_hash = hash_storage_slot_leaf(&address, &attestation_hash, &value).unwrap();
/// ```
pub fn hash_storage_slot(
    address: &Address,
    attestation_hash: &U256,
    //value: &U256,
    value: &FixedBytes<32>,
) -> Result<Fp> {
    let address_slice = address.as_slice();
    let att_hash_bytes = attestation_hash.to_le_bytes::<32>();
    //let value_slice = value.to_be_bytes::<32>();//value.as_slice();
    let value_slice = value.as_slice();
    print!("{:?} 0x", address);
    for b in attestation_hash.to_be_bytes::<32>().iter() {
        print!("{:02x}", b);
    }
    println!(" {:?}", value);

    let mut first_field_bytes = [0u8; 32];
    first_field_bytes[0..20].copy_from_slice(&address_slice[0..20]);
    first_field_bytes[20] = att_hash_bytes[0];
    first_field_bytes[21] = value_slice[0];

    let mut second_field_bytes = [0u8; 32];
    second_field_bytes[0..31].copy_from_slice(&att_hash_bytes[1..32]);

    let mut third_field_bytes = [0u8; 32];
    third_field_bytes[0..31].copy_from_slice(&value_slice[1..32]);

    let first_field = Fp::from_bytes(&first_field_bytes)?;
    let second_field = Fp::from_bytes(&second_field_bytes)?;
    let third_field = Fp::from_bytes(&third_field_bytes)?;

    println!("first_field {:?}", first_field);
    println!("second_field {:?}", second_field);
    println!("third_field {:?}", third_field);

    Ok(poseidon_hash(&[first_field, second_field, third_field]))
}

#[cfg(test)]
mod merkle_fixed_tests {
    use super::*;
    use alloy_primitives::{hex::FromHex, Address, FixedBytes};
    use anyhow::Result;

    /*
    // Helper: Generate dummy Address with bytes all set to given value
    fn dummy_address(byte: u8) -> Address {
        let bytes = [byte; 20];
        Address::from_slice(&bytes)
    }

    // Helper: Generate dummy U256 with bytes all set to given value
    fn dummy_attestation(byte: u8) -> U256 {
        U256::from_le_bytes([byte; 32])
    }

    // Helper: Generate dummy FixedBytes<32> with bytes all set to given value
    fn dummy_value(byte: u8) -> FixedBytes<32> {
        FixedBytes::<32>::new([byte; 32])
    }
    */

    fn dummy_address(i: i32) -> Address {
        let mut bytes = [0u8; 20];
        let i_bytes = i.to_le_bytes();
        bytes[16..20].copy_from_slice(&i_bytes);
        Address::from_slice(&bytes)
    }

    fn dummy_attestation(i: i32) -> U256 {
        let mut bytes = [0u8; 32];
        let i_bytes = i.to_le_bytes();
        bytes[0..4].copy_from_slice(&i_bytes);
        U256::from_le_bytes(bytes)
    }

    fn dummy_value(i: i32) -> FixedBytes<32> {
        let mut bytes = [0u8; 32];
        let i_bytes = i.to_le_bytes();
        bytes[0..4].copy_from_slice(&i_bytes);
        FixedBytes::<32>::new(bytes)
    }

    // Build leaf hashes from given (address, attestation_hash, value) tuples
    fn build_leaves(triples: &[(Address, U256, FixedBytes<32>)]) -> Result<Vec<Fp>> {
        let mut leaves = Vec::with_capacity(triples.len());
        for (addr, att_hash, val) in triples {
            leaves.push(hash_storage_slot(addr, att_hash, val)?);
        }
        Ok(leaves)
    }

    // Full Merkle lifecycle test using your actual hashed leaves
    fn full_merkle_test(
        triples: &[(Address, U256, FixedBytes<32>)],
        leaf_index: usize,
    ) -> Result<()> {
        let zeros = get_merkle_zeros();
        let leaves = build_leaves(triples)?;
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
        let slot_key_address_str = "c7e910807dd2e3f49b34efe7133cfb684520da69";
        let slot_nested_key_attestation_hash_str =
            "6500000000000000000000000000000000000000000000000000000000000000";
        let value_str = "2ba7def3000";

        // Convert slot_key_address_str to Address ([u8; 20])
        let mut address_bytes = [0u8; 20];
        for i in 0..20 {
            let byte_str = &slot_key_address_str[i * 2..i * 2 + 2];
            address_bytes[i] = u8::from_str_radix(byte_str, 16).unwrap();
        }
        let address = Address::from(address_bytes);

        let mut hash_bytes = [0u8; 32];
        for i in 0..32 {
            let byte_str = &slot_nested_key_attestation_hash_str[i * 2..i * 2 + 2];
            hash_bytes[31 - i] = u8::from_str_radix(byte_str, 16).unwrap(); // reverse into LE
        }
        let attestation_hash = U256::from_le_bytes(hash_bytes);

        let mut value_bytes = [0u8; 32];

        /*// Convert value_str to FixedBytes<32>, right-padded with zeros

        let value_len = value_str.len() / 2;
        for i in 0..value_len {
            let byte_str = &value_str[i * 2..i * 2 + 2];
            value_bytes[i] = u8::from_str_radix(byte_str, 16).unwrap();
        }*/

        // Convert value_str to FixedBytes<32>, left-padded with zeros
        let mut value_bytes = [0u8; 32];
        let value_len = value_str.len() / 2;
        for i in 0..value_len {
            let byte_str = &value_str[i * 2..i * 2 + 2];
            value_bytes[32 - value_len + i] = u8::from_str_radix(byte_str, 16).unwrap();
        }

        let value = FixedBytes::<32>::from(&value_bytes);

        print!("{:?} 0x", address);
        for b in attestation_hash.to_be_bytes::<32>().iter() {
            print!("{:02x}", b);
        }
        println!(" {:?}", value);
        // Call hash_storage_slot function
        let result = hash_storage_slot(&address, &attestation_hash, &value).unwrap();

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
        let triples: Vec<(Address, U256, FixedBytes<32>)> = (0..n)
            .map(|i| (dummy_address(i), dummy_attestation(i), dummy_value(i)))
            .collect();
        full_merkle_test(&triples, 543)
    }

    #[test]
    fn test_hash_storage_slot_basic() -> Result<()> {
        let address = dummy_address(1);
        let att_hash = dummy_attestation(2);
        let value = dummy_value(3);
        let leaf_hash = hash_storage_slot(&address, &att_hash, &value)?;
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
            let triples: Vec<(Address, U256, FixedBytes<32>)> = (0..n_leaves)
                .map(|i| (dummy_address(i), dummy_attestation(i), dummy_value(i)))
                .collect();

            let leaves = build_leaves(&triples).expect("build_leaves failed");
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
