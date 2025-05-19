use alloy::sol;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    NoriStateBridge,
    "../nori-contracts/artifacts/contracts/NoriTokenBridge.sol/NoriTokenBridge.json"
);