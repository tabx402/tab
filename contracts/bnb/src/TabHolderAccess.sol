// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {TabProtocol} from "./TabProtocol.sol";

/// @dev Eligibility is checked when new risk is taken, never when funds are recovered.
library TabHolderAccess {
    error TabHoldingRequired();

    function eligible(TabProtocol protocol, address account) internal view returns (bool) {
        address token = protocol.tabToken();
        if (account == address(0) || token.code.length == 0) return false;
        try IERC20(token).balanceOf(account) returns (uint256 amount) {
            return amount > 0;
        } catch {
            return false;
        }
    }

    function requireHolder(TabProtocol protocol, address account) internal view {
        if (!eligible(protocol, account)) revert TabHoldingRequired();
    }
}
