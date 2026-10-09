// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {TabUSDTLiquidity} from "./TabUSDTLiquidity.sol";
import {TabTypes as T} from "./TabTypes.sol";
import {ITabPriceOracle} from "./TabPriceOracle.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";

/// @notice Pooled, native-BNB-secured, zero-interest USDT working capital for a funded Tab job.
/// @dev The owner accepts every line and explicitly pledges BNB. Lenders bear residual liquidation loss.
/// The existing immutable escrow pays its executor normally; repayment is explicit, not intercepted.
contract TabLendingPool is TabUSDTLiquidity {
    uint64 public constant LOSS_GRACE = 7 days;
    uint64 public constant LIQUIDATION_GRACE = 1 days;
    address public constant WBNB = 0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c;
    uint16 public constant LTV_BPS = 5000;
    uint16 public constant LIQUIDATION_BPS = 7500;
    uint16 public constant LIQUIDATION_BONUS_BPS = 500;
    ITabPriceOracle public immutable collateralOracle;
    mapping(bytes32 => uint256) public collateral;
    uint256 public totalCollateral;

    struct LoanTerms {
        bytes32 id;
        bytes32 agent;
        bytes32 job;
        address signer;
        uint256 principal;
        uint256 perCall;
        uint256 dailyCap;
        uint64 expiresAt;
        uint64 tools;
        address[] recipients;
    }

    struct Loan {
        bytes32 agent;
        bytes32 job;
        address borrower;
        address signer;
        uint256 principal;
        uint256 available;
        uint256 debt;
        uint256 loss;
        uint256 spent;
        uint256 repaid;
        uint256 perCall;
        uint256 dailyCap;
        uint256 dailySpent;
        uint64 spendDay;
        uint64 expiresAt;
        uint64 tools;
        bool accepted;
        bool closed;
        address[] recipients;
    }

    mapping(bytes32 => Loan) private loans;
    mapping(bytes32 => bytes32) public jobLoan;
    mapping(bytes32 => mapping(bytes32 => T.Payment)) private receipts;
    mapping(bytes32 => uint256) public agentDaySpent;
    mapping(bytes32 => uint64) public agentSpendDay;

    event LoanApproved(
        bytes32 indexed loan, bytes32 indexed agent, bytes32 indexed job, address borrower, uint256 principal
    );
    event LoanAccepted(bytes32 indexed loan);
    event WorkingCapitalPaid(
        bytes32 indexed loan,
        address indexed recipient,
        uint256 amount,
        uint64 tool,
        bytes32 request,
        bytes32 receipt
    );
    event LoanRepaid(bytes32 indexed loan, address indexed payer, uint256 principal);
    event LoanClosed(bytes32 indexed loan, uint256 released);
    event CollateralPledged(bytes32 indexed loan, address indexed borrower, uint256 amount);
    event CollateralWithdrawn(bytes32 indexed loan, address indexed borrower, uint256 amount);
    event LoanLiquidated(bytes32 indexed loan, address indexed liquidator, uint256 repaid, uint256 seized);
    error Collateral();

    constructor(address protocol_, address collateralOracle_)
        TabUSDTLiquidity(protocol_, "Tab secured working capital USDT", "tabWC")
    {
        collateralOracle = ITabPriceOracle(collateralOracle_);
        if (collateralOracle_.code.length == 0 || collateralOracle.token() != WBNB
            || collateralOracle.quoteToken() != address(usdt) || collateralOracle.price() == 0) revert Collateral();
    }

    function securedCreditVersion() external pure returns (uint256) { return 1; }

    /// @return value USDT value, borrowing maximum debt, liquidationDebt threshold for liquidation.
    function collateralQuote(uint256 amount) public view returns (uint256 value, uint256 borrowing, uint256 liquidationDebt) {
        uint256 price = collateralOracle.price();
        if (price == 0) revert Collateral();
        value = Math.mulDiv(amount, price, 1 ether);
        borrowing = Math.mulDiv(value, LTV_BPS, 10_000);
        liquidationDebt = Math.mulDiv(value, LIQUIDATION_BPS, 10_000);
    }

    function loanHealth(bytes32 id) external view returns (uint256 value, uint256 borrowing, uint256 liquidationDebt, bool liquidatable) {
        (value, borrowing, liquidationDebt) = collateralQuote(collateral[id]);
        Loan storage l = loans[id];
        liquidatable = l.debt > 0 && (l.debt > liquidationDebt || block.timestamp > uint256(l.expiresAt) + LIQUIDATION_GRACE);
    }

    /// Risk-reducing top-ups remain available during pauses, holder loss and oracle failure.
    function pledgeCollateral(bytes32 id) external payable nonReentrant {
        Loan storage l = loans[id];
        if (msg.sender != l.borrower || msg.value == 0 || (l.closed && l.debt == 0)) revert Authority();
        collateral[id] += msg.value;
        totalCollateral += msg.value;
        emit CollateralPledged(id, msg.sender, msg.value);
    }

    /// Debt-free exit never depends on the oracle, the TAB balance, or job status.
    function withdrawCollateral(bytes32 id, uint256 amount) external nonReentrant {
        Loan storage l = loans[id];
        if (msg.sender != l.borrower || amount == 0 || amount > collateral[id]) revert Authority();
        collateral[id] -= amount;
        if (l.debt > 0) {
            (,uint256 borrowing,) = collateralQuote(collateral[id]);
            if (l.debt > borrowing) revert Collateral();
        }
        totalCollateral -= amount;
        _payNative(msg.sender, amount);
        emit CollateralWithdrawn(id, msg.sender, amount);
    }

    /// The requested USDT amount is a maximum. Insolvent liquidation repays only the exact collateral quote.
    function liquidationQuote(bytes32 id, uint256 maximum) public view returns (uint256 repaid, uint256 seized) {
        Loan storage l = loans[id];
        if (maximum == 0 || maximum > l.debt || collateral[id] == 0) revert Terms();
        uint256 price = collateralOracle.price();
        if (price == 0) revert Collateral();
        uint256 value = Math.mulDiv(collateral[id], price, 1 ether);
        if (l.debt <= Math.mulDiv(value, LIQUIDATION_BPS, 10_000)
            && block.timestamp <= uint256(l.expiresAt) + LIQUIDATION_GRACE) revert Collateral();
        uint256 coveredDebt = Math.mulDiv(value, 10_000, 10_000 + LIQUIDATION_BONUS_BPS);
        repaid = Math.min(maximum, coveredDebt);
        if (repaid == 0) revert Collateral();
        if (maximum >= coveredDebt) seized = collateral[id];
        else {
            uint256 withBonus = Math.mulDiv(repaid, 10_000 + LIQUIDATION_BONUS_BPS, 10_000, Math.Rounding.Ceil);
            seized = Math.min(collateral[id], Math.mulDiv(withBonus, 1 ether, price, Math.Rounding.Ceil));
        }
        if (seized == 0) revert Collateral();
    }

    function liquidateLoan(bytes32 id, uint256 maximum, uint256 minimumCollateral) external nonReentrant {
        (uint256 repaid, uint256 seized) = liquidationQuote(id, maximum);
        if (minimumCollateral == 0 || seized < minimumCollateral) revert Collateral();
        Loan storage l = loans[id];
        _close(id, l);
        uint256 recoveredLoss = Math.min(repaid, l.loss);
        l.loss -= recoveredLoss;
        l.debt -= repaid;
        l.repaid += repaid;
        collateral[id] -= seized;
        totalCollateral -= seized;
        _repay(msg.sender, repaid, repaid - recoveredLoss);
        _payNative(msg.sender, seized);
        emit LoanLiquidated(id, msg.sender, repaid, seized);
    }

    function _payNative(address recipient, uint256 amount) private {
        (bool ok,) = recipient.call{value: amount}("");
        if (!ok) revert TransferFailed();
    }

    function getLoan(bytes32 id) external view returns (Loan memory) {
        return loans[id];
    }

    function getReceipt(bytes32 id, bytes32 request) external view returns (T.Payment memory) {
        return receipts[id][request];
    }

    function approveLoan(LoanTerms calldata t) external {
        if (msg.sender != underwriter) revert Authority();
        T.Agent memory a = protocol.getAgent(t.agent);
        _requireTabHolder(a.owner);
        T.Job memory j = protocol.getJob(t.job);
        if (
            paused || t.id == 0 || loans[t.id].borrower != address(0) || jobLoan[t.job] != 0
                || a.owner == address(0) || a.paused || j.executor != a.owner || j.agent != t.agent
                || j.state != 1 || j.paused || j.root == 0 || protocol.getJob(j.root).paused
                || t.principal == 0 || t.principal > j.available || t.principal > protocol.MAX_BUDGET()
                || t.principal > availableLiquidity() || t.perCall == 0 || t.perCall > t.dailyCap
                || t.dailyCap > t.principal || t.dailyCap > a.dailyCap || t.perCall > j.maxCall
                || t.expiresAt <= block.timestamp || t.expiresAt > j.deadline
                || t.expiresAt > block.timestamp + 31 days || t.signer == address(0) || t.tools == 0
                || (t.tools & ~j.tools) != 0 || t.recipients.length == 0 || t.recipients.length > 16
        ) revert Terms();
        for (uint256 i; i < t.recipients.length; i++) {
            address recipient = t.recipients[i];
            if (
                recipient == address(0) || recipient == address(this) || recipient == address(protocol)
                    || recipient == a.owner || recipient == t.signer || recipient == underwriter
            ) revert Terms();
            bool jobAllowed;
            for (uint256 k; k < j.recipients.length; k++) {
                if (j.recipients[k] == recipient) jobAllowed = true;
            }
            if (!jobAllowed) revert Terms();
            for (uint256 k; k < i; k++) {
                if (t.recipients[k] == recipient) revert Terms();
            }
        }
        Loan storage l = loans[t.id];
        l.agent = t.agent;
        l.job = t.job;
        l.borrower = a.owner;
        l.signer = t.signer;
        l.principal = t.principal;
        l.available = t.principal;
        l.perCall = t.perCall;
        l.dailyCap = t.dailyCap;
        l.expiresAt = t.expiresAt;
        l.tools = t.tools;
        l.recipients = t.recipients;
        jobLoan[t.job] = t.id;
        reserved += t.principal;
        emit LoanApproved(t.id, t.agent, t.job, a.owner, t.principal);
    }

    function acceptLoan(bytes32 id) external {
        _requireTabHolder(msg.sender);
        Loan storage l = loans[id];
        if (msg.sender != l.borrower) revert Authority();
        if (paused || l.closed || l.accepted || block.timestamp >= l.expiresAt) revert Terms();
        _active(l);
        l.accepted = true;
        emit LoanAccepted(id);
    }

    function _active(Loan storage l) internal view returns (T.Agent memory a, T.Job memory j) {
        _requireTabHolder(l.borrower);
        a = protocol.getAgent(l.agent);
        j = protocol.getJob(l.job);
        if (
            a.owner != l.borrower || a.paused || j.executor != l.borrower || j.agent != l.agent
                || j.state != 1 || j.paused || protocol.getJob(j.root).paused || block.timestamp >= j.deadline
        ) revert Terms();
    }

    function spendLoan(
        bytes32 id,
        address recipient,
        uint256 amount,
        uint64 tool,
        bytes32 request,
        bytes32 receipt
    ) external nonReentrant {
        Loan storage l = loans[id];
        if (msg.sender != l.borrower && msg.sender != l.signer) revert Authority();
        if (
            paused || !l.accepted || l.closed || block.timestamp >= l.expiresAt || amount == 0
                || amount > l.available || amount > l.perCall || request == 0 || receipt == 0
                || receipts[id][request].paidAt != 0 || tool == 0 || (tool & (tool - 1)) != 0
                || (tool & ~l.tools) != 0
        ) revert Terms();
        (T.Agent memory a, T.Job memory j) = _active(l);
        if (amount > j.maxCall || (tool & ~j.tools) != 0) revert Terms();
        bool allowed;
        bool jobAllowed;
        for (uint256 i; i < l.recipients.length; i++) {
            if (l.recipients[i] == recipient) allowed = true;
        }
        for (uint256 i; i < j.recipients.length; i++) {
            if (j.recipients[i] == recipient) jobAllowed = true;
        }
        if (!allowed || !jobAllowed) revert Authority();
        (,uint256 borrowing,) = collateralQuote(collateral[id]);
        if (l.debt + amount > borrowing) revert Collateral();
        uint64 day = uint64(block.timestamp / 1 days);
        if (l.spendDay != day) {
            l.spendDay = day;
            l.dailySpent = 0;
        }
        if (agentSpendDay[l.agent] != day) {
            agentSpendDay[l.agent] = day;
            agentDaySpent[l.agent] = 0;
        }
        l.dailySpent += amount;
        agentDaySpent[l.agent] += amount;
        // Includes protocol spending already recorded, but immutable protocol cannot see this module's spend.
        uint256 protocolSpent = a.spendDay == day ? a.dailySpent : 0;
        if (l.dailySpent > l.dailyCap || agentDaySpent[l.agent] + protocolSpent > a.dailyCap) revert Terms();
        l.available -= amount;
        reserved -= amount;
        l.debt += amount;
        l.spent += amount;
        receipts[id][request] = T.Payment(recipient, amount, tool, uint64(block.timestamp), receipt);
        _lend(recipient, amount);
        emit WorkingCapitalPaid(id, recipient, amount, tool, request, receipt);
    }

    /// Repayment reduces debt rather than reopening the spend line. Anyone may repay for an agent.
    function repayLoan(bytes32 id, uint256 amount) external nonReentrant {
        Loan storage l = loans[id];
        if (amount == 0 || amount > l.debt) revert Terms();
        uint256 recoveredLoss = amount < l.loss ? amount : l.loss;
        l.loss -= recoveredLoss;
        l.debt -= amount;
        l.repaid += amount;
        _repay(msg.sender, amount, amount - recoveredLoss);
        emit LoanRepaid(id, msg.sender, amount);
    }

    function closeLoan(bytes32 id) external {
        Loan storage l = loans[id];
        if (l.borrower == address(0) || l.closed) revert Terms();
        T.Job memory j = protocol.getJob(l.job);
        bool expired = block.timestamp >= l.expiresAt || j.state >= 3;
        if (msg.sender != l.borrower && msg.sender != underwriter && !expired) revert Authority();
        _close(id, l);
    }

    function _close(bytes32 id, Loan storage l) private {
        if (l.closed) return;
        uint256 released = l.available;
        l.available = 0;
        l.closed = true;
        reserved -= released;
        emit LoanClosed(id, released);
    }

    /// Loss is transparent and never forgives the borrower's obligation. Later recovery accrues to current shareholders.
    function recognizeLoss(bytes32 id) external {
        if (msg.sender != underwriter) revert Authority();
        Loan storage l = loans[id];
        if (!l.closed || collateral[id] != 0 || block.timestamp < uint256(l.expiresAt) + LOSS_GRACE || l.debt <= l.loss) {
            revert Terms();
        }
        uint256 loss = l.debt - l.loss;
        l.loss = l.debt;
        outstanding -= loss;
        emit LossRecognized(id, loss);
    }
}
