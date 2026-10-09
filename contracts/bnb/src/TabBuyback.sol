// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {TabProtocol} from "./TabProtocol.sol";
import {TabHolderAccess} from "./TabHolderAccess.sol";

interface ITabSwapRouter {
    function getAmountsOut(uint256 amountIn, address[] calldata path)
        external
        view
        returns (uint256[] memory amounts);
    function swapExactTokensForTokens(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] calldata path,
        address to,
        uint256 deadline
    ) external returns (uint256[] memory amounts);
}

/// @notice Explicitly funded USDT buybacks through a fixed router and official-token route.
/// @dev This module cannot withdraw fee reserves from the immutable deployed protocol.
/// Tokens are sent to the fixed dead address. It claims neither supply reduction nor automatic fee funding.
contract TabBuyback is ReentrancyGuard {
    using SafeERC20 for IERC20;
    address public constant DEAD = 0x000000000000000000000000000000000000dEaD;
    TabProtocol public immutable protocol;
    IERC20 public immutable usdt;
    IERC20 public immutable token;
    ITabSwapRouter public immutable router;
    address public immutable operator;
    uint16 public immutable maximumSlippageBps;
    address[] private route;
    uint256 public fundedUsdt;
    uint256 public spentUsdt;
    uint256 public tokensAcquired;
    bool public paused;

    struct BuybackView {
        address asset;
        address token;
        address router;
        address operator;
        uint16 maximumSlippageBps;
        bool paused;
        uint256 fundedUsdt;
        uint256 spentUsdt;
        uint256 tokensAcquired;
        address[] route;
    }
    error Terms();
    error Authority();
    error TransferFailed();
    event BuybackFunded(address indexed funder, uint256 amount);
    event BuybackExecuted(
        uint256 usdtSpent, uint256 tokensAcquired, uint256 quotedTokens, uint256 minimumTokens
    );
    event BuybackPaused(bool paused);

    constructor(address protocol_, address router_, address[] memory route_, uint16 maxSlippageBps_) {
        protocol = TabProtocol(protocol_);
        usdt = protocol.usdt();
        address official = protocol.tabToken();
        if (
            official.code.length == 0 || router_.code.length == 0 || maxSlippageBps_ > 1000
                || route_.length < 2 || route_.length > 4 || route_[0] != address(usdt)
                || route_[route_.length - 1] != official
        ) revert Terms();
        for (uint256 i; i < route_.length; i++) {
            if (route_[i].code.length == 0) revert Terms();
            for (uint256 j; j < i; j++) {
                if (route_[j] == route_[i]) revert Terms();
            }
        }
        token = IERC20(official);
        router = ITabSwapRouter(router_);
        operator = protocol.authority();
        maximumSlippageBps = maxSlippageBps_;
        route = route_;
    }

    function getRoute() external view returns (address[] memory) {
        return route;
    }

    function getBuyback() external view returns (BuybackView memory) {
        return BuybackView(
            address(usdt),
            address(token),
            address(router),
            operator,
            maximumSlippageBps,
            paused,
            fundedUsdt,
            spentUsdt,
            tokensAcquired,
            route
        );
    }

    function availableUsdt() public view returns (uint256) {
        return fundedUsdt - spentUsdt;
    }

    /// Funding is an explicit, irreversible contribution to the stated buyback route.
    function fund(uint256 amount) external nonReentrant {
        TabHolderAccess.requireHolder(protocol, msg.sender);
        if (amount == 0 || amount > protocol.MAX_BUDGET()) revert Terms();
        uint256 before_ = usdt.balanceOf(address(this));
        uint256 senderBefore = usdt.balanceOf(msg.sender);
        usdt.safeTransferFrom(msg.sender, address(this), amount);
        if (
            usdt.balanceOf(address(this)) - before_ != amount
                || senderBefore - usdt.balanceOf(msg.sender) != amount
        ) revert TransferFailed();
        fundedUsdt += amount;
        emit BuybackFunded(msg.sender, amount);
    }

    function setPaused(bool value) external {
        if (msg.sender != operator) revert Authority();
        paused = value;
        emit BuybackPaused(value);
    }

    function quote(uint256 amount) external view returns (uint256 quoted, uint256 minimum) {
        quoted = _quote(amount);
        minimum = Math.mulDiv(quoted, 10_000 - maximumSlippageBps, 10_000, Math.Rounding.Ceil);
    }

    function _quote(uint256 amount) internal view returns (uint256 output) {
        if (amount == 0 || amount > protocol.MAX_BUDGET()) revert Terms();
        uint256[] memory amounts = router.getAmountsOut(amount, route);
        if (amounts.length != route.length || amounts[0] != amount) revert Terms();
        output = amounts[amounts.length - 1];
        if (output == 0) revert Terms();
    }

    /// Only the designated operator executes; a quote is a slippage bound, not a manipulation-resistant oracle.
    function execute(uint256 amount, uint256 minimumTokens, uint256 deadline) external nonReentrant {
        if (msg.sender != operator) revert Authority();
        TabHolderAccess.requireHolder(protocol, msg.sender);
        if (
            paused || amount == 0 || amount > availableUsdt() || deadline < block.timestamp
                || deadline > block.timestamp + 10 minutes
        ) revert Terms();
        uint256 quoted = _quote(amount);
        uint256 floor = Math.mulDiv(quoted, 10_000 - maximumSlippageBps, 10_000, Math.Rounding.Ceil);
        if (minimumTokens < floor || minimumTokens == 0) revert Terms();
        uint256 beforeUsdt = usdt.balanceOf(address(this));
        uint256 beforeTokens = token.balanceOf(address(this));
        usdt.forceApprove(address(router), amount);
        uint256[] memory swapped =
            router.swapExactTokensForTokens(amount, minimumTokens, route, address(this), deadline);
        usdt.forceApprove(address(router), 0);
        if (
            beforeUsdt - usdt.balanceOf(address(this)) != amount || swapped.length != route.length
                || swapped[0] != amount
        ) revert TransferFailed();
        uint256 received = token.balanceOf(address(this)) - beforeTokens;
        if (received < minimumTokens || received != swapped[swapped.length - 1]) revert TransferFailed();
        spentUsdt += amount;
        tokensAcquired += received;
        uint256 beforeDead = token.balanceOf(DEAD);
        uint256 burnBefore = token.balanceOf(address(this));
        token.safeTransfer(DEAD, received);
        if (
            token.balanceOf(DEAD) - beforeDead != received
                || burnBefore - token.balanceOf(address(this)) != received
        ) revert TransferFailed();
        emit BuybackExecuted(amount, received, quoted, minimumTokens);
    }
}
