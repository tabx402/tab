// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {ITabPriceOracle} from "./TabPriceOracle.sol";
import {TabProtocol} from "./TabProtocol.sol";
import {TabTypes as T} from "./TabTypes.sol";

/// @notice Exact-unit custody and isolated, collateral-secured, zero-interest USDT credit.
/// @dev A custody deposit is separate from an explicitly pledged credit position.
contract TabBacking is ReentrancyGuard {
    using SafeERC20 for IERC20;
    TabProtocol public immutable protocol;
    IERC20 public immutable usdt;
    mapping(bytes32 => mapping(address => mapping(address => T.Backing))) private balances;
    mapping(address => uint256) public tokenLiability;
    uint256 public nativeLiability;
    mapping(bytes32 => T.Credit) private credits;
    mapping(bytes32 => mapping(bytes32 => T.Payment)) private payments;
    struct CollateralAsset {
        address oracle;
        uint16 ltvBps;
        uint16 liquidationBps;
        uint16 bonusBps;
        uint8 decimals;
        bool paused;
        uint256 debtCap;
        uint256 debt;
    }
    struct CollateralPosition { address token; uint256 amount; }
    mapping(address => CollateralAsset) public collateralAssets;
    mapping(bytes32 => CollateralPosition) public collateralPositions;
    error Collateral();
    event CollateralConfigured(address indexed token, address oracle, uint16 ltvBps, uint16 liquidationBps, uint16 bonusBps, uint256 debtCap);
    event CollateralPledged(bytes32 indexed credit, address indexed borrower, address indexed token, uint256 amount);
    event CollateralReleased(bytes32 indexed credit, uint256 amount);
    event CreditLiquidated(bytes32 indexed credit, address indexed liquidator, uint256 repaid, uint256 seized);
    error Terms();
    error Authority();
    error TransferFailed();
    event Backed(
        bytes32 indexed agent, address indexed backer, address indexed token, uint256 amount, uint8 decimals
    );
    event BackingWithdrawn(
        bytes32 indexed agent, address indexed backer, address indexed token, uint256 amount
    );
    event CreditOpened(
        bytes32 indexed credit,
        bytes32 indexed agent,
        address indexed lender,
        address borrower,
        uint256 amount
    );
    event CreditAccepted(bytes32 indexed credit);
    event CreditPaid(
        bytes32 indexed credit,
        address indexed recipient,
        uint256 amount,
        uint64 tool,
        bytes32 requestHash,
        bytes32 receiptHash
    );
    event CreditRepaid(bytes32 indexed credit, address indexed payer, uint256 amount);
    event CreditWithdrawn(bytes32 indexed credit, uint256 amount);
    event CreditClosed(bytes32 indexed credit);

    constructor(address protocol_) {
        protocol = TabProtocol(protocol_);
        usdt = protocol.usdt();
        // Identical collateral/debt token: no external USD peg assumption is needed.
        collateralAssets[address(usdt)] = CollateralAsset(address(0), 9000, 9500, 200, 18, false, 1_000_000 ether, 0);
    }

    /// @dev Immutable risk terms per token. Authority can pause new debt, never seize assets.
    function configureCollateral(address token, address oracle, uint16 ltv, uint16 threshold, uint16 bonus, uint256 cap) external {
        if (msg.sender != protocol.authority()) revert Authority();
        if (collateralAssets[token].ltvBps != 0 || token.code.length == 0 || oracle.code.length == 0
            || ltv == 0 || ltv > 7000 || threshold <= ltv || threshold > 8500 || bonus > 1000
            || uint256(threshold) * (10_000 + bonus) >= 100_000_000 || cap == 0 || cap > protocol.MAX_BUDGET()
            || ITabPriceOracle(oracle).token() != token || ITabPriceOracle(oracle).quoteToken() != address(usdt)) revert Terms();
        uint8 decimals = IERC20Metadata(token).decimals();
        if (decimals > 18 || ITabPriceOracle(oracle).price() == 0) revert Terms();
        collateralAssets[token] = CollateralAsset(oracle, ltv, threshold, bonus, decimals, false, cap, 0);
        emit CollateralConfigured(token, oracle, ltv, threshold, bonus, cap);
    }

    function pauseCollateral(address token, bool paused) external {
        if (msg.sender != protocol.authority() || collateralAssets[token].ltvBps == 0) revert Authority();
        collateralAssets[token].paused = paused;
    }

    function collateralPrice(address token) public view returns (uint256) {
        CollateralAsset storage a = collateralAssets[token];
        if (a.ltvBps == 0) revert Collateral();
        return token == address(usdt) ? 1 ether : ITabPriceOracle(a.oracle).price();
    }

    function collateralValue(bytes32 id) public view returns (uint256) {
        CollateralPosition storage position = collateralPositions[id];
        return Math.mulDiv(position.amount, collateralPrice(position.token), 10 ** collateralAssets[position.token].decimals);
    }

    function borrowingPower(bytes32 id) public view returns (uint256) {
        return Math.mulDiv(collateralValue(id), collateralAssets[collateralPositions[id].token].ltvBps, 10_000);
    }

    function pledgeCollateral(bytes32 id, uint256 amount) external nonReentrant {
        T.Credit storage c = credits[id];
        if (msg.sender != c.borrower || amount == 0) revert Authority();
        CollateralPosition storage position = collateralPositions[id];
        position.amount += amount;
        _pull(IERC20(position.token), msg.sender, amount);
        emit CollateralPledged(id, msg.sender, position.token, amount);
    }

    function withdrawCollateral(bytes32 id, uint256 amount) external nonReentrant {
        T.Credit storage c = credits[id];
        CollateralPosition storage position = collateralPositions[id];
        if (msg.sender != c.borrower || amount == 0 || amount > position.amount) revert Authority();
        position.amount -= amount;
        // A repaid position can always exit, including during oracle failures or pauses.
        if (c.outstanding > 0 && c.outstanding > borrowingPower(id)) revert Collateral();
        _pay(IERC20(position.token), msg.sender, amount);
        emit CollateralReleased(id, amount);
    }

    /// @notice Repay debt for collateral at the fixed bonus; the lender receives actual USDT.
    /// @dev After expiry borrowers have a 24-hour repayment grace period. No oracle bypass.
    function liquidateCredit(bytes32 id, uint256 amount, uint256 minCollateralOut) external nonReentrant {
        T.Credit storage c = credits[id];
        CollateralPosition storage position = collateralPositions[id];
        CollateralAsset storage asset = collateralAssets[position.token];
        if (amount == 0 || amount > c.outstanding) revert Terms();
        uint256 price = collateralPrice(position.token);
        uint256 value = Math.mulDiv(position.amount, price, 10 ** asset.decimals);
        if (c.outstanding <= Math.mulDiv(value, asset.liquidationBps, 10_000)
            && block.timestamp <= uint256(c.expiresAt) + 1 days) revert Collateral();
        uint256 debtWithBonus = Math.mulDiv(amount, 10_000 + asset.bonusBps, 10_000, Math.Rounding.Ceil);
        uint256 seized = Math.mulDiv(debtWithBonus, 10 ** asset.decimals, price, Math.Rounding.Ceil);
        if (seized == 0 || seized > position.amount || seized < minCollateralOut) revert Collateral();
        c.outstanding -= amount;
        c.available += amount;
        c.totalRepaid += amount;
        c.closed = true;
        asset.debt -= amount;
        position.amount -= seized;
        _pull(usdt, msg.sender, amount);
        _pay(IERC20(position.token), msg.sender, seized);
        emit CreditLiquidated(id, msg.sender, amount, seized);
    }

    function getBacking(bytes32 agent, address backer, address token)
        external
        view
        returns (T.Backing memory)
    {
        return balances[agent][backer][token];
    }

    function getCredit(bytes32 id) external view returns (T.Credit memory) {
        return credits[id];
    }

    function getCreditPayment(bytes32 id, bytes32 request) external view returns (T.Payment memory) {
        return payments[id][request];
    }

    function _agent(bytes32 id) internal view returns (T.Agent memory a) {
        a = protocol.getAgent(id);
        if (a.owner == address(0)) revert Terms();
    }

    function _pull(IERC20 token, address from, uint256 amount) internal {
        uint256 before_ = token.balanceOf(address(this));
        token.safeTransferFrom(from, address(this), amount);
        if (token.balanceOf(address(this)) - before_ != amount) revert TransferFailed();
        tokenLiability[address(token)] += amount;
    }

    function _pay(IERC20 token, address to, uint256 amount) internal {
        uint256 before_ = token.balanceOf(to);
        tokenLiability[address(token)] -= amount;
        token.safeTransfer(to, amount);
        if (token.balanceOf(to) - before_ != amount) revert TransferFailed();
    }

    function backAgent(bytes32 agent, address token, uint256 amount) external nonReentrant {
        _agent(agent);
        if (token.code.length == 0 || amount == 0) revert Terms();
        uint8 decimals = IERC20Metadata(token).decimals();
        if (decimals > 18) revert Terms();
        T.Backing storage b = balances[agent][msg.sender][token];
        b.amount += amount;
        b.decimals = decimals;
        _pull(IERC20(token), msg.sender, amount);
        emit Backed(agent, msg.sender, token, amount, decimals);
    }

    function withdrawBacking(bytes32 agent, address token, uint256 amount) external nonReentrant {
        T.Backing storage b = balances[agent][msg.sender][token];
        if (token == address(0) || amount == 0 || amount > b.amount) revert Terms();
        b.amount -= amount;
        _pay(IERC20(token), msg.sender, amount);
        emit BackingWithdrawn(agent, msg.sender, token, amount);
    }

    function backBNB(bytes32 agent) external payable nonReentrant {
        _agent(agent);
        if (msg.value == 0) revert Terms();
        T.Backing storage b = balances[agent][msg.sender][address(0)];
        b.amount += msg.value;
        b.decimals = 18;
        nativeLiability += msg.value;
        emit Backed(agent, msg.sender, address(0), msg.value, 18);
    }

    function withdrawBNB(bytes32 agent, uint256 amount) external nonReentrant {
        T.Backing storage b = balances[agent][msg.sender][address(0)];
        if (amount == 0 || amount > b.amount) revert Terms();
        b.amount -= amount;
        nativeLiability -= amount;
        (bool ok,) = msg.sender.call{value: amount}("");
        if (!ok) revert TransferFailed();
        emit BackingWithdrawn(agent, msg.sender, address(0), amount);
    }

    /// @dev Lender fixes all spend terms. Borrower must explicitly accept before anyone may spend.
    function openCredit(T.CreditTerms calldata t) external nonReentrant {
        _openCredit(t, address(usdt));
    }

    function openCreditWithCollateral(T.CreditTerms calldata t, address token) external nonReentrant {
        _openCredit(t, token);
    }

    function _openCredit(T.CreditTerms calldata t, address token) internal {
        if (collateralAssets[token].ltvBps == 0 || collateralAssets[token].paused) revert Collateral();
        T.Agent memory a = _agent(t.agent);
        if (
            t.id == 0 || credits[t.id].lender != address(0) || t.amount == 0
                || t.amount > protocol.MAX_BUDGET() || t.perCall == 0 || t.perCall > t.dailyCap
                || t.dailyCap > t.amount || t.expiresAt <= block.timestamp
                || t.expiresAt > block.timestamp + 31 days || t.signer == address(0) || t.tools == 0
                || t.recipients.length == 0 || t.recipients.length > 16
        ) revert Terms();
        for (uint256 i; i < t.recipients.length; i++) {
            address r = t.recipients[i];
            if (
                r == address(0) || r == address(this) || r == address(protocol) || r == a.owner
                    || r == t.signer || r == msg.sender
            ) revert Terms();
            for (uint256 j; j < i; j++) {
                if (r == t.recipients[j]) revert Terms();
            }
        }
        T.Credit storage c = credits[t.id];
        c.agent = t.agent;
        c.lender = msg.sender;
        c.borrower = a.owner;
        c.signer = t.signer;
        c.funded = t.amount;
        c.available = t.amount;
        c.perCall = t.perCall;
        c.dailyCap = t.dailyCap;
        c.expiresAt = t.expiresAt;
        c.tools = t.tools;
        c.recipients = t.recipients;
        collateralPositions[t.id].token = token;
        _pull(usdt, msg.sender, t.amount);
        emit CreditOpened(t.id, t.agent, msg.sender, a.owner, t.amount);
    }

    function acceptCredit(bytes32 id) external {
        T.Credit storage c = credits[id];
        if (msg.sender != c.borrower || c.closed || block.timestamp >= c.expiresAt) revert Authority();
        c.accepted = true;
        emit CreditAccepted(id);
    }

    function closeCredit(bytes32 id) external {
        T.Credit storage c = credits[id];
        if (msg.sender != c.lender && msg.sender != c.borrower) revert Authority();
        c.closed = true;
        emit CreditClosed(id);
    }

    function spendCredit(
        bytes32 id,
        address recipient,
        uint256 amount,
        uint64 tool,
        bytes32 request,
        bytes32 receipt
    ) external nonReentrant {
        T.Credit storage c = credits[id];
        if (
            (msg.sender != c.borrower && msg.sender != c.signer) || !c.accepted || c.closed
                || block.timestamp >= c.expiresAt || amount == 0 || amount > c.available || amount > c.perCall
                || request == 0 || receipt == 0 || payments[id][request].paidAt != 0 || tool == 0
                || (tool & (tool - 1)) != 0 || (tool & ~c.tools) != 0
        ) revert Terms();
        bool allowed;
        for (uint256 i; i < c.recipients.length; i++) {
            if (c.recipients[i] == recipient) allowed = true;
        }
        if (!allowed) revert Authority();
        CollateralAsset storage asset = collateralAssets[collateralPositions[id].token];
        if (asset.paused || c.outstanding + amount > borrowingPower(id) || asset.debt + amount > asset.debtCap) revert Collateral();
        asset.debt += amount;
        uint64 day = uint64(block.timestamp / 1 days);
        if (c.spendDay != day) {
            c.spendDay = day;
            c.dailySpent = 0;
        }
        c.dailySpent += amount;
        if (c.dailySpent > c.dailyCap) revert Terms();
        c.available -= amount;
        c.outstanding += amount;
        c.totalSpent += amount;
        payments[id][request] = T.Payment(recipient, amount, tool, uint64(block.timestamp), receipt);
        protocol.consumeCreditDaily(c.agent, c.borrower, amount);
        _pay(usdt, recipient, amount);
        emit CreditPaid(id, recipient, amount, tool, request, receipt);
    }

    function repayCredit(bytes32 id, uint256 amount) external nonReentrant {
        T.Credit storage c = credits[id];
        if (amount == 0 || amount > c.outstanding) revert Terms();
        c.outstanding -= amount;
        c.available += amount;
        c.totalRepaid += amount;
        collateralAssets[collateralPositions[id].token].debt -= amount;
        _pull(usdt, msg.sender, amount);
        emit CreditRepaid(id, msg.sender, amount);
    }

    /// @dev Only unspent or repaid principal can be withdrawn. Outstanding credit remains at risk.
    function withdrawCredit(bytes32 id, uint256 amount) external nonReentrant {
        T.Credit storage c = credits[id];
        if (msg.sender != c.lender) revert Authority();
        if (amount == 0 || amount > c.available) revert Terms();
        c.available -= amount;
        c.withdrawn += amount;
        _pay(usdt, msg.sender, amount);
        emit CreditWithdrawn(id, amount);
    }
}
