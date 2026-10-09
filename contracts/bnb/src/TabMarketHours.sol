// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

/// @notice Short-lived, operator-attested market status for a specific collateral token.
/// @dev Closed by default. A status is an attestation, not an autonomous exchange calendar.
/// The operator must obtain current trading/transfer availability from the asset issuer.
contract TabMarketHours {
    uint64 public constant MAX_VALIDITY = 15 minutes;
    address public immutable operator;

    struct Status {
        bool open;
        uint64 validUntil;
        uint64 updatedAt;
        bytes32 sourceHash;
    }
    mapping(address => Status) public statuses;
    error Terms();
    error Authority();
    event MarketStatusPublished(address indexed collateral, bool open, uint64 validUntil, bytes32 sourceHash);

    constructor(address operator_) {
        if (operator_ == address(0)) revert Terms();
        operator = operator_;
    }

    function publish(address collateral, bool open, uint64 validUntil, bytes32 sourceHash) external {
        if (msg.sender != operator) revert Authority();
        if (collateral.code.length == 0 || sourceHash == 0) revert Terms();
        if (open && (validUntil <= block.timestamp || validUntil > block.timestamp + MAX_VALIDITY)) {
            revert Terms();
        }
        if (!open) validUntil = uint64(block.timestamp);
        statuses[collateral] = Status(open, validUntil, uint64(block.timestamp), sourceHash);
        emit MarketStatusPublished(collateral, open, validUntil, sourceHash);
    }

    function isOpen(address collateral) external view returns (bool) {
        Status memory s = statuses[collateral];
        return s.open && s.validUntil > block.timestamp;
    }
}
