# Historical credit experiment

These sources and tests preserve an experimental rewrite of the already deployed, non-upgradeable TabProtocol and TabBacking contracts. They are excluded from the active Foundry source and test directories and are not supported by the existing BNB deployment addresses.

This snapshot predates the v2 migration. The active deployment in `contracts/deployments/bnb-56.json` now uses 0.5% fees and collateralized direct credit. Its current source and ABIs are in `contracts/bnb/src` and `contracts/bnb/abi`. The original 2% unsecured deployment is retained in `contracts/deployments/bnb-56-legacy.json` for historical verification.

Do not use this historical snapshot for deployment or runtime ABI loading. The separate pooled job-advance module remains inactive because it permits unsecured loans. Stock collateral remains disabled until its exact BNB token oracle and lending policy are verified.

The original deployment's sources were restored from these Sourcify exact-match records for chain 56. These addresses describe the historical deployment, not the current v2 modules:

- [TabProtocol](https://sourcify.dev/server/v2/contract/56/0x567e7187d477b1a68c3ac3d292ad44a0d5e770c7?fields=all)
- [TabBacking](https://sourcify.dev/server/v2/contract/56/0x31033e00e7e9d8c050560c69a0cb31b4135a79b4?fields=all)
- [TabEconomics](https://sourcify.dev/server/v2/contract/56/0xd3963f0f2bf05ae36ae8eadc682b88f9660db7c3?fields=all)
