# Changelog

## 28/9/26 - Ethereum state settlement on Solana

Adapt the bridge head to settle Ethereum's finalized state on Solana.

- `nori-hash/src/merkle_poseidon_fixed.rs`: renamed to `nori-hash/src/merkle_sha256_fixed.rs`. The verified requests tree and request leaf hash with SHA-256, which Solana can recompute, in place of Kimchi Poseidon. `pack_request_leaf_fields` and `hash_request_leaf` are infallible.
- `nori-hash/src/merkle-zeros.dat`: regenerated with SHA-256 zero hashes.
- `nori-hash/src/lib.rs`: exports `merkle_sha256_fixed`.
- `nori-hash/src/bin/hash_vectors.rs`: emits SHA-256 vectors.
- `nori-hash/scripts/cross_check.mjs`: checks the vectors against Node `crypto` SHA-256.
- `nori-hash/scripts/package.json`: `o1js` dependency removed.
- `nori-hash/tests/request_leaf_byte_routing.rs`: asserts packing on 32-byte fields and writes leaves as hex.
- `test-vectors/proof-request-queue/request-leaf-vectors.json`: regenerated.
- `test-vectors/proof-request-queue/README.md`: documents SHA-256 leaf packing and names `nori-solana-sdk` and the Solana program as consumers.
- `nori-program/src/mpt.rs`: builds the requests root with `merkle_sha256_fixed`. `MptError::LeafHashError` removed, as leaf hashing is infallible.
- `nori-program/src/consensus.rs`: `LeafHashError` removed from the error documentation; the verifier contract comment names the destination chain.
- `nori/bin/nori_bridge_head.rs`: the queue cursor comment names the destination chain.
- `nori/src/rpcs/execution/http.rs`: imports `MAX_BATCH` from `merkle_sha256_fixed`.
- `nori/tests/benchmark_sha256_serde.rs`: sends `ProofInputs`, the type the guest decodes, and asserts exit code 0.
- `.env.example`: `SP1_PROOF_TYPE=groth16`, the proof type verified on Solana.
- `README.md`: network proving examples use `groth16`; the contract deployment link points to `nori-solana-sdk`.
- `Cargo.toml`, `nori-hash/Cargo.toml`, `nori-program/Cargo.toml`, `Cargo.lock`: `mina-poseidon`, `mina-curves` and `o1-utils` removed.
