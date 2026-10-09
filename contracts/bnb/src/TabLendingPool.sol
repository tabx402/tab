// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {TabUSDTLiquidity} from "./TabUSDTLiquidity.sol";
import {TabTypes as T} from "./TabTypes.sol";

/// @notice Pooled, unsecured, zero-interest USDT working capital for a specific funded Tab job.
/// @dev The underwriter approves risk; the agent owner accepts every line. Lenders can lose principal.
/// The existing immutable escrow pays its executor normally; repayment is explicit, not intercepted.
contract TabLendingPool is TabUSDTLiquidity {
    uint64 public constant LOSS_GRACE = 7 days;

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

    constructor(address protocol_) TabUSDTLiquidity(protocol_, "Tab working capital USDT", "tabWC") {}

    function getLoan(bytes32 id) external view returns (Loan memory) {
        return loans[id];
    }

    function getReceipt(bytes32 id, bytes32 request) external view returns (T.Payment memory) {
        return receipts[id][request];
    }

    function approveLoan(LoanTerms calldata t) external {
        if (msg.sender != underwriter) revert Authority();
        T.Agent memory a = protocol.getAgent(t.agent);
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
        Loan storage l = loans[id];
        if (msg.sender != l.borrower) revert Authority();
        if (paused || l.closed || l.accepted || block.timestamp >= l.expiresAt) revert Terms();
        _active(l);
        l.accepted = true;
        emit LoanAccepted(id);
    }

    function _active(Loan storage l) internal view returns (T.Agent memory a, T.Job memory j) {
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
        if (!l.closed || block.timestamp < uint256(l.expiresAt) + LOSS_GRACE || l.debt <= l.loss) {
            revert Terms();
        }
        uint256 loss = l.debt - l.loss;
        l.loss = l.debt;
        outstanding -= loss;
        emit LossRecognized(id, loss);
    }
}
