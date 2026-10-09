// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {ERC4626} from "@openzeppelin/contracts/token/ERC20/extensions/ERC4626.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {TabProtocol} from "./TabProtocol.sol";
import {TabHolderAccess} from "./TabHolderAccess.sol";

/// @dev Principal-only ERC4626 pool. Outstanding loans are receivables, not withdrawable cash.
/// Direct token donations are excluded from accounting. Governance cannot withdraw lender assets.
abstract contract TabUSDTLiquidity is ERC4626, ReentrancyGuard {
    using SafeERC20 for IERC20;

    TabProtocol public immutable protocol;
    IERC20 public immutable usdt;
    address public immutable underwriter;
    uint256 public liquidity;
    uint256 public outstanding;
    uint256 public reserved;
    bool public paused;

    struct PoolView {
        address asset;
        address underwriter;
        bool paused;
        uint256 liquidity;
        uint256 reserved;
        uint256 outstanding;
        uint256 assets;
        uint256 shares;
    }

    error Terms();
    error Authority();
    error TransferFailed();
    event LendingPaused(bool paused);
    event LossRecognized(bytes32 indexed loan, uint256 principal);

    constructor(address protocol_, string memory name_, string memory symbol_)
        ERC20(name_, symbol_)
        ERC4626(IERC20(address(TabProtocol(protocol_).usdt())))
    {
        protocol = TabProtocol(protocol_);
        usdt = protocol.usdt();
        underwriter = protocol.authority();
        if (address(usdt).code.length == 0 || IERC20Metadata(address(usdt)).decimals() != 18) revert Terms();
    }

    function totalAssets() public view override returns (uint256) {
        return liquidity + outstanding;
    }

    function holderGateVersion() external pure returns (uint256) { return 1; }

    function hasTabAccess(address account) public view returns (bool) {
        return TabHolderAccess.eligible(protocol, account);
    }

    function _requireTabHolder(address account) internal view {
        TabHolderAccess.requireHolder(protocol, account);
    }

    function availableLiquidity() public view returns (uint256) {
        return liquidity - reserved;
    }

    function getPool() external view returns (PoolView memory) {
        return PoolView(
            address(usdt), underwriter, paused, liquidity, reserved, outstanding, totalAssets(), totalSupply()
        );
    }

    function maxWithdraw(address owner) public view override returns (uint256) {
        return Math.min(super.maxWithdraw(owner), availableLiquidity());
    }

    function maxDeposit(address receiver) public view override returns (uint256) {
        return hasTabAccess(receiver) ? protocol.MAX_BUDGET() : 0;
    }

    function maxMint(address receiver) public view override returns (uint256) {
        return hasTabAccess(receiver) ? _convertToShares(protocol.MAX_BUDGET(), Math.Rounding.Floor) : 0;
    }

    function maxRedeem(address owner) public view override returns (uint256) {
        return Math.min(balanceOf(owner), _convertToShares(availableLiquidity(), Math.Rounding.Floor));
    }

    function deposit(uint256 assets, address receiver) public override nonReentrant returns (uint256) {
        _requireTabHolder(msg.sender);
        _requireTabHolder(receiver);
        if (assets == 0 || receiver == address(0) || receiver == address(this)) revert Terms();
        return super.deposit(assets, receiver);
    }

    function mint(uint256 shares, address receiver) public override nonReentrant returns (uint256) {
        _requireTabHolder(msg.sender);
        _requireTabHolder(receiver);
        if (shares == 0 || receiver == address(0) || receiver == address(this)) revert Terms();
        return super.mint(shares, receiver);
    }

    function withdraw(uint256 assets, address receiver, address owner)
        public
        override
        nonReentrant
        returns (uint256)
    {
        if (assets == 0 || receiver == address(0) || receiver == address(this)) revert Terms();
        return super.withdraw(assets, receiver, owner);
    }

    function redeem(uint256 shares, address receiver, address owner)
        public
        override
        nonReentrant
        returns (uint256)
    {
        if (shares == 0 || receiver == address(0) || receiver == address(this)) revert Terms();
        return super.redeem(shares, receiver, owner);
    }

    function _deposit(address caller, address receiver, uint256 assets, uint256 shares) internal override {
        if (shares == 0) revert Terms();
        uint256 before_ = usdt.balanceOf(address(this));
        uint256 senderBefore = usdt.balanceOf(caller);
        super._deposit(caller, receiver, assets, shares);
        if (
            usdt.balanceOf(address(this)) - before_ != assets
                || senderBefore - usdt.balanceOf(caller) != assets
        ) revert TransferFailed();
        liquidity += assets;
    }

    function _withdraw(address caller, address receiver, address owner, uint256 assets, uint256 shares)
        internal
        override
    {
        if (assets > availableLiquidity()) revert Terms();
        liquidity -= assets;
        uint256 before_ = usdt.balanceOf(receiver);
        uint256 poolBefore = usdt.balanceOf(address(this));
        super._withdraw(caller, receiver, owner, assets, shares);
        if (
            usdt.balanceOf(receiver) - before_ != assets
                || poolBefore - usdt.balanceOf(address(this)) != assets
        ) {
            revert TransferFailed();
        }
    }

    /// Pausing stops new credit. Repayment, collateral top-ups and lender redemption remain available.
    function setPaused(bool value) external {
        if (msg.sender != underwriter) revert Authority();
        paused = value;
        emit LendingPaused(value);
    }

    function _pull(IERC20 token, address from, uint256 amount) internal {
        uint256 before_ = token.balanceOf(address(this));
        uint256 senderBefore = token.balanceOf(from);
        token.safeTransferFrom(from, address(this), amount);
        if (
            token.balanceOf(address(this)) - before_ != amount
                || senderBefore - token.balanceOf(from) != amount
        ) revert TransferFailed();
    }

    function _pay(IERC20 token, address to, uint256 amount) internal {
        if (to == address(0) || to == address(this)) revert Terms();
        uint256 before_ = token.balanceOf(to);
        uint256 poolBefore = token.balanceOf(address(this));
        token.safeTransfer(to, amount);
        if (token.balanceOf(to) - before_ != amount || poolBefore - token.balanceOf(address(this)) != amount)
        {
            revert TransferFailed();
        }
    }

    function _lend(address recipient, uint256 amount) internal {
        if (amount == 0 || amount > availableLiquidity()) revert Terms();
        liquidity -= amount;
        outstanding += amount;
        _pay(usdt, recipient, amount);
    }

    function _repay(address payer, uint256 amount, uint256 recognizedPrincipal) internal {
        _pull(usdt, payer, amount);
        liquidity += amount;
        outstanding -= recognizedPrincipal;
    }
}
