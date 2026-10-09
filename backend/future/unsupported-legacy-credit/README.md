# Unsupported legacy credit experiment

`financial.rs` preserves a parallel experiment that expected collateral methods
on the existing BNB backing address. It is outside `backend/src` and is not
compiled or included in release packages.

This snapshot predates the v2 migration. The current deployment manifest is
`contracts/deployments/bnb-56.json`; its backing contract supports separately
funded, collateralized, zero-interest credit. Active preparation and recovery
live in `backend/src` and use the current verified interfaces. Stock collateral
lending belongs to the separate `TabStockLending` module and requires its own
verified deployment and asset policy.

Related experimental contracts and ABIs are preserved under
`contracts/bnb/future/unsupported-legacy-credit`.
