// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import {SignatureChecker} from "@openzeppelin/contracts/utils/cryptography/SignatureChecker.sol";
import {TabTypes as T} from "./TabTypes.sol";

interface ITabModule {
    function protocol() external view returns (address);
}

interface ITabEconomics {
    function pairedToken(bytes32 id) external view returns (address);
}

/// @notice Non-upgradeable BNB execution budgets and USDT job escrows. No administrator withdrawal.
contract TabProtocol is EIP712, ReentrancyGuard {
    using SafeERC20 for IERC20;
    uint256 public constant MAX_BUDGET = 10_000 ether;
    uint64 public constant REVIEW = 1 days;
    address public constant BNB_USDT = 0x55d398326f99059fF775485246999027B3197955;
    bytes32 public constant REGISTER_TYPEHASH = keccak256(
        "Register(bytes32 id,address owner,string name,uint256 dailyCap,bytes32 policyHash,uint256 nonce,uint256 deadline)"
    );
    IERC20 public immutable usdt;
    address public immutable authority;
    address public backing;
    address public economics;
    address public tabToken;
    address public legacyProtocol;
    uint16 public constant feeBps = 50;
    uint256 public completedWorkUsdt;
    uint256 public feesCollectedUsdt;
    uint256 public totalLiability;
    mapping(address => uint256) public registrationNonces;
    mapping(bytes32 => T.Agent) private agents;
    mapping(bytes32 => T.Job) private jobs;
    mapping(bytes32 => T.Spending) private spending;
    mapping(bytes32 => T.Session) private sessions;
    mapping(bytes32 => mapping(bytes32 => T.Payment)) private payments;
    mapping(bytes32 => mapping(bytes32 => bool)) public jobRequests;

    error Terms();
    error Authority();
    error DailyCap();
    error TokenTransfer();
    event AgentRegistered(
        bytes32 indexed agent, address indexed owner, string name, uint256 dailyCap, bytes32 policyHash
    );
    event PolicyUpdated(bytes32 indexed agent, uint256 dailyCap, bytes32 policyHash, uint32 version);
    event AgentPaused(bytes32 indexed agent, bool paused);
    event ModulesConfigured(address backing, address economics);
    event TabConfigured(address token);
    event LegacyConfigured(address indexed previous);
    event AgentImported(bytes32 indexed agent, address indexed previous);
    event SpendingFunded(bytes32 indexed agent, address indexed funder, uint256 amount);
    event SpendingWithdrawn(bytes32 indexed agent, uint256 amount);
    event SessionGranted(bytes32 indexed session, bytes32 indexed agent, address signer);
    event SessionRevoked(bytes32 indexed session);
    event SessionPayment(
        bytes32 indexed session,
        bytes32 indexed agent,
        address indexed recipient,
        uint256 amount,
        bytes32 requestHash,
        bytes32 receiptHash
    );
    event JobOpened(
        bytes32 indexed job, bytes32 indexed parent, address indexed buyer, address executor, uint256 budget
    );
    event JobBound(bytes32 indexed job, bytes32 indexed agent, address executor);
    event JobSubmitted(bytes32 indexed job, bytes32 evidence);
    event JobRejected(bytes32 indexed job);
    event JobPaused(bytes32 indexed root, bool paused);
    event CallPaid(
        bytes32 indexed job,
        address indexed recipient,
        uint256 amount,
        uint64 tool,
        bytes32 requestHash,
        bytes32 receiptHash
    );
    event JobPaid(bytes32 indexed job, uint256 gross, uint256 net, uint256 fee);
    event JobCancelled(bytes32 indexed job, uint256 refunded);
    event BranchClosed(bytes32 indexed job);

    constructor(address usdt_, address authority_) EIP712("Tab Protocol", "1") {
        if (authority_ == address(0) || usdt_.code.length == 0 || IERC20Metadata(usdt_).decimals() != 18) {
            revert Terms();
        }
        // Production is BNB only. Local tests explicitly use chain 31337 and mock USDT.
        if (block.chainid != 31337 && (block.chainid != 56 || usdt_ != BNB_USDT)) revert Terms();
        usdt = IERC20(usdt_);
        authority = authority_;
    }

    function configureModules(address b, address e) external {
        if (msg.sender != authority || backing != address(0) || economics != address(0)) revert Authority();
        if (b == e || ITabModule(b).protocol() != address(this) || ITabModule(e).protocol() != address(this)) revert Terms();
        backing = b;
        economics = e;
        emit ModulesConfigured(b, e);
    }

    /// @notice Pin the previous registry before sealing this deployment's modules.
    function configureLegacy(address previous) external {
        if (msg.sender != authority || legacyProtocol != address(0) || backing != address(0)
            || economics != address(0)) revert Authority();
        if (previous == address(this) || previous.code.length == 0) revert Terms();
        T.Protocol memory prior = TabProtocol(previous).getProtocol();
        if (prior.authority != authority || prior.usdt != address(usdt)
            || prior.backing == address(0) || prior.economics == address(0)) revert Terms();
        legacyProtocol = previous;
        emit LegacyConfigured(previous);
    }

    /// @notice Copy existing identities exactly. This grants no payment sessions and moves no assets.
    /// @dev Anyone may relay an import; the original owner and policy are read from the pinned registry.
    function importAgents(bytes32[] calldata ids) external nonReentrant {
        if (legacyProtocol == address(0) || backing == address(0) || ids.length == 0 || ids.length > 100) revert Terms();
        for (uint256 i; i < ids.length; ++i) {
            bytes32 id = ids[i];
            if (agents[id].owner != address(0)) revert Terms();
            T.Agent memory prior = TabProtocol(legacyProtocol).getAgent(id);
            if (prior.owner == address(0) || address(bytes20(id)) != prior.owner) revert Terms();
            _policy(prior.dailyCap, prior.policyHash);
            agents[id] = prior;
            emit AgentRegistered(id, prior.owner, prior.name, prior.dailyCap, prior.policyHash);
            if (prior.version > 1) emit PolicyUpdated(id, prior.dailyCap, prior.policyHash, prior.version);
            if (prior.paused) emit AgentPaused(id, true);
            emit AgentImported(id, legacyProtocol);
        }
    }

    /// @dev Records an existing official token once. Does not mint a replacement TAB token.
    function configureTab(address token) external {
        if (msg.sender != authority || tabToken != address(0)) revert Authority();
        if (token.code.length == 0 || token == address(usdt) || IERC20Metadata(token).decimals() > 18) {
            revert Terms();
        }
        tabToken = token;
        emit TabConfigured(token);
    }

    /// @notice The recipient of completed-work earnings pays no Tab fee while holding official TAB.
    /// @dev Evaluate at settlement, so selling a token after funding does not retain an exemption.
    function effectiveFeeBps(address recipient) public view returns (uint16) {
        if (tabToken != address(0) && IERC20(tabToken).balanceOf(recipient) > 0) return 0;
        return feeBps;
    }

    function getProtocol() external view returns (T.Protocol memory) {
        return T.Protocol(
            authority,
            address(usdt),
            tabToken,
            backing,
            economics,
            feeBps,
            completedWorkUsdt,
            feesCollectedUsdt,
            0,
            0
        );
    }

    function getAgent(bytes32 id) external view returns (T.Agent memory) {
        return agents[id];
    }

    function getJob(bytes32 id) external view returns (T.Job memory) {
        return jobs[id];
    }

    function getSpending(bytes32 id) external view returns (T.Spending memory) {
        return spending[id];
    }

    function getSession(bytes32 id) external view returns (T.Session memory) {
        return sessions[id];
    }

    function getPayment(bytes32 id, bytes32 request) external view returns (T.Payment memory) {
        return payments[id][request];
    }

    function sessionId(bytes32 agent, uint64 nonce) public pure returns (bytes32) {
        return keccak256(abi.encode(agent, nonce));
    }

    function register(bytes32 id, string calldata name, uint256 cap, bytes32 policy) external {
        _register(id, msg.sender, name, cap, policy);
    }

    function registerWithSignature(
        bytes32 id,
        address owner,
        string calldata name,
        uint256 cap,
        bytes32 policy,
        uint256 nonce,
        uint256 deadline,
        bytes calldata signature
    ) external {
        if (block.timestamp > deadline || nonce != registrationNonces[owner]++) {
            revert Authority();
        }
        bytes32 digest = _hashTypedDataV4(
            keccak256(
                abi.encode(REGISTER_TYPEHASH, id, owner, keccak256(bytes(name)), cap, policy, nonce, deadline)
            )
        );
        if (!SignatureChecker.isValidSignatureNow(owner, digest, signature)) revert Authority();
        _register(id, owner, name, cap, policy);
    }

    function _register(bytes32 id, address owner, string calldata name, uint256 cap, bytes32 policy)
        internal
    {
        if (
            id == 0 || owner == address(0) || address(bytes20(id)) != owner || agents[id].owner != address(0)
                || bytes(name).length < 2 || bytes(name).length > 48
        ) revert Terms();
        if (legacyProtocol != address(0) && TabProtocol(legacyProtocol).getAgent(id).owner != address(0)) revert Terms();
        _policy(cap, policy);
        agents[id] = T.Agent(owner, name, cap, policy, false, 1, 0, 0);
        emit AgentRegistered(id, owner, name, cap, policy);
    }

    function _policy(uint256 cap, bytes32 policy) internal pure {
        if (cap == 0 || cap > MAX_BUDGET || policy == 0) revert Terms();
    }

    function _own(bytes32 id) internal view {
        if (agents[id].owner != msg.sender) revert Authority();
    }

    function updatePolicy(bytes32 id, uint256 cap, bytes32 policy) external {
        _own(id);
        _policy(cap, policy);
        T.Agent storage a = agents[id];
        a.dailyCap = cap;
        a.policyHash = policy;
        ++a.version;
        emit PolicyUpdated(id, cap, policy, a.version);
    }

    function pauseAgent(bytes32 id, bool paused) external {
        _own(id);
        agents[id].paused = paused;
        emit AgentPaused(id, paused);
    }

    function _consume(bytes32 id, uint256 amount) internal {
        T.Agent storage a = agents[id];
        if (a.owner == address(0) || a.paused) revert Authority();
        uint64 day = uint64(block.timestamp / 1 days);
        if (a.spendDay != day) {
            a.spendDay = day;
            a.dailySpent = 0;
        }
        a.dailySpent += amount;
        if (a.dailySpent > a.dailyCap) revert DailyCap();
    }

    function consumeCreditDaily(bytes32 id, address borrower, uint256 amount) external {
        if (msg.sender != backing || borrower != agents[id].owner || amount == 0) revert Authority();
        _consume(id, amount);
    }

    function _pull(address from, uint256 amount) internal {
        uint256 before_ = usdt.balanceOf(address(this));
        usdt.safeTransferFrom(from, address(this), amount);
        if (usdt.balanceOf(address(this)) - before_ != amount) revert TokenTransfer();
        totalLiability += amount;
    }

    function _pay(address recipient, uint256 amount) internal {
        if (amount == 0) return;
        if (recipient == address(this) || recipient == address(0)) revert Terms();
        totalLiability -= amount;
        usdt.safeTransfer(recipient, amount);
    }

    function fundSpending(bytes32 id, uint256 amount) external nonReentrant {
        if (agents[id].owner == address(0) || amount == 0) revert Terms();
        T.Spending storage s = spending[id];
        s.available += amount;
        s.totalFunded += amount;
        _pull(msg.sender, amount);
        emit SpendingFunded(id, msg.sender, amount);
    }

    function withdrawSpending(bytes32 id, uint256 amount) external nonReentrant {
        _own(id);
        T.Spending storage s = spending[id];
        if (amount == 0 || amount > s.available) revert Terms();
        s.available -= amount;
        s.totalWithdrawn += amount;
        _pay(msg.sender, amount);
        emit SpendingWithdrawn(id, amount);
    }

    function grantSession(bytes32 id, T.SessionTerms calldata t) external {
        _own(id);
        bytes32 sid = sessionId(id, t.nonce);
        if (
            sessions[sid].owner != address(0) || t.signer == address(0) || t.expiresAt <= block.timestamp
                || t.expiresAt > block.timestamp + 31 days || t.perCall == 0 || t.perCall > t.dailyCap
                || t.dailyCap > t.totalCap || t.totalCap > MAX_BUDGET || t.tools == 0
        ) revert Terms();
        _recipients(t.recipients);
        T.Session storage s = sessions[sid];
        s.agent = id;
        s.owner = msg.sender;
        s.signer = t.signer;
        s.nonce = t.nonce;
        s.expiresAt = t.expiresAt;
        s.perCall = t.perCall;
        s.dailyCap = t.dailyCap;
        s.totalCap = t.totalCap;
        s.tools = t.tools;
        s.recipients = t.recipients;
        emit SessionGranted(sid, id, t.signer);
    }

    function _recipients(address[] calldata recipients) internal view {
        if (recipients.length == 0 || recipients.length > 16) revert Terms();
        for (uint256 i; i < recipients.length; i++) {
            if (recipients[i] == address(0) || recipients[i] == address(this)) revert Terms();
            for (uint256 n; n < i; n++) {
                if (recipients[n] == recipients[i]) revert Terms();
            }
        }
    }

    function revokeSession(bytes32 sid) external {
        T.Session storage s = sessions[sid];
        if (msg.sender != s.owner) revert Authority();
        s.revoked = true;
        emit SessionRevoked(sid);
    }

    function sessionPay(
        bytes32 sid,
        address recipient,
        uint256 amount,
        uint64 tool,
        bytes32 request,
        bytes32 receipt
    ) external nonReentrant {
        T.Session storage s = sessions[sid];
        if (
            msg.sender != s.signer || s.revoked || block.timestamp >= s.expiresAt || amount == 0
                || amount > s.perCall || request == 0 || receipt == 0 || payments[sid][request].paidAt != 0
        ) revert Authority();
        _tool(tool, s.tools);
        bool allowed;
        for (uint256 i; i < s.recipients.length; i++) {
            if (s.recipients[i] == recipient) allowed = true;
        }
        if (!allowed) revert Authority();
        uint64 day = uint64(block.timestamp / 1 days);
        if (s.spendDay != day) {
            s.spendDay = day;
            s.dailySpent = 0;
        }
        s.dailySpent += amount;
        s.totalSpent += amount;
        if (s.dailySpent > s.dailyCap || s.totalSpent > s.totalCap) revert DailyCap();
        _consume(s.agent, amount);
        T.Spending storage f = spending[s.agent];
        if (amount > f.available) revert Terms();
        f.available -= amount;
        f.totalSpent += amount;
        payments[sid][request] = T.Payment(recipient, amount, tool, uint64(block.timestamp), receipt);
        _pay(recipient, amount);
        emit SessionPayment(sid, s.agent, recipient, amount, request, receipt);
    }

    function _tool(uint64 tool, uint64 tools) internal pure {
        if (tool == 0 || (tool & (tool - 1)) != 0 || (tool & ~tools) != 0) revert Terms();
    }

    function _terms(T.JobTerms calldata t) internal view {
        if (
            t.id == 0 || jobs[t.id].state != 0 || t.budget == 0 || t.budget > MAX_BUDGET || t.maxCall == 0
                || t.maxCall > t.budget || t.deadline <= block.timestamp
                || t.deadline > block.timestamp + 31 days || t.termsHash == 0 || t.executor == address(this)
        ) revert Terms();
        if (t.recipients.length > 0) _recipients(t.recipients);
        for (uint256 i; i < t.recipients.length; ++i) {
            if (t.recipients[i] == t.executor || t.recipients[i] == msg.sender) revert Terms();
        }
    }

    function openJob(T.JobTerms calldata t) external nonReentrant {
        _terms(t);
        T.Job storage j = jobs[t.id];
        j.root = t.id;
        j.buyer = msg.sender;
        _newJob(j, t);
        _pull(msg.sender, t.budget);
        emit JobOpened(t.id, 0, msg.sender, t.executor, t.budget);
    }

    function _newJob(T.Job storage j, T.JobTerms calldata t) internal {
        j.executor = t.executor;
        j.budget = t.budget;
        j.available = t.budget;
        j.maxCall = t.maxCall;
        j.deadline = t.deadline;
        j.termsHash = t.termsHash;
        j.tools = t.tools;
        j.state = 1;
        j.feeBps = feeBps;
        j.recipients = t.recipients;
    }

    function delegateJob(bytes32 parent, T.JobTerms calldata t) external {
        _terms(t);
        T.Job storage p = jobs[parent];
        if (
            p.state != 1 || p.executor != msg.sender || jobs[p.root].paused || p.depth >= 8
                || t.budget > p.available || t.maxCall > p.maxCall || t.deadline > p.deadline
                || (t.tools & ~p.tools) != 0
        ) revert Terms();
        for (uint256 i; i < t.recipients.length; ++i) {
            bool allowed;
            for (uint256 n; n < p.recipients.length; ++n) {
                if (t.recipients[i] == p.recipients[n]) allowed = true;
            }
            if (!allowed) revert Terms();
        }
        p.available -= t.budget;
        ++p.children;
        T.Job storage j = jobs[t.id];
        j.root = p.root;
        j.parent = parent;
        j.buyer = msg.sender;
        j.depth = p.depth + 1;
        _newJob(j, t);
        emit JobOpened(t.id, parent, msg.sender, t.executor, t.budget);
    }

    function bindJobAgent(bytes32 job, bytes32 agent) external {
        _own(agent);
        T.Job storage j = jobs[job];
        if (
            j.state != 1 || j.executor != msg.sender || j.agent != 0 || jobs[j.root].paused
                || agents[agent].paused || block.timestamp > j.deadline
        ) revert Authority();
        j.agent = agent;
        emit JobBound(job, agent, msg.sender);
    }

    function claimJob(bytes32 job, bytes32 agent) external {
        _own(agent);
        T.Job storage j = jobs[job];
        if (
            j.state != 1 || j.executor != address(0) || j.agent != 0 || jobs[j.root].paused
                || agents[agent].paused || block.timestamp >= j.deadline || tabToken == address(0)
                || economics == address(0)
        ) revert Authority();
        address paired = ITabEconomics(economics).pairedToken(agent);
        if (
            paired == address(0) || IERC20(paired).balanceOf(msg.sender) == 0
                || IERC20(tabToken).balanceOf(msg.sender) == 0
        ) revert Authority();
        j.agent = agent;
        j.executor = msg.sender;
        emit JobBound(job, agent, msg.sender);
    }

    function submitJob(bytes32 id, bytes32 evidence) external {
        T.Job storage j = jobs[id];
        if (j.state != 1 || j.executor != msg.sender || block.timestamp > j.deadline || evidence == 0) {
            revert Authority();
        }
        j.evidence = evidence;
        j.state = 2;
        if (!j.timelySubmitted) j.submittedAt = uint64(block.timestamp);
        j.timelySubmitted = true;
        emit JobSubmitted(id, evidence);
    }

    function rejectJob(bytes32 id) external {
        T.Job storage j = jobs[id];
        if (
            j.state != 2 || jobs[j.root].buyer != msg.sender || jobs[j.root].paused
                || block.timestamp > reviewEnd(id)
        ) {
            revert Authority();
        }
        j.evidence = 0;
        j.state = 1;
        emit JobRejected(id);
    }

    function pauseJob(bytes32 root, bool paused) external {
        T.Job storage j = jobs[root];
        if (j.buyer != msg.sender || j.root != root) revert Authority();
        j.paused = paused;
        emit JobPaused(root, paused);
    }

    function payCall(
        bytes32 id,
        address recipient,
        uint256 amount,
        uint64 tool,
        bytes32 request,
        bytes32 receipt
    ) external nonReentrant {
        T.Job storage j = jobs[id];
        if (
            j.state != 1 || j.executor != msg.sender || agents[j.agent].owner != msg.sender
                || jobs[j.root].paused || amount == 0 || amount > j.available || amount > j.maxCall
                || block.timestamp > j.deadline || request == 0 || receipt == 0 || jobRequests[id][request]
        ) revert Terms();
        bool allowed;
        for (uint256 i; i < j.recipients.length; ++i) {
            if (j.recipients[i] == recipient) allowed = true;
        }
        if (!allowed || recipient == j.executor || recipient == j.buyer) revert Authority();
        _tool(tool, j.tools);
        _consume(j.agent, amount);
        jobRequests[id][request] = true;
        j.available -= amount;
        j.providerSpent += amount;
        _pay(recipient, amount);
        emit CallPaid(id, recipient, amount, tool, request, receipt);
    }

    function acceptJob(bytes32 id, bytes32 evidence) external nonReentrant {
        T.Job storage j = jobs[id];
        if (
            j.state != 2 || jobs[j.root].buyer != msg.sender || jobs[j.root].paused || j.children != 0
                || j.evidence != evidence || evidence == 0 || block.timestamp > reviewEnd(id)
        ) revert Authority();
        uint256 gross = j.available;
        uint256 fee = gross * effectiveFeeBps(j.executor) / 10_000;
        uint256 net = gross - fee;
        j.available = 0;
        j.rewardPaid = net;
        j.feePaid = fee;
        j.state = 3;
        completedWorkUsdt += gross;
        feesCollectedUsdt += fee;
        _pay(j.executor, net);
        // Fee units remain in immutable custody as a reserve. No claimed buyback or treasury drain.
        emit JobPaid(id, gross, net, fee);
    }

    function reviewEnd(bytes32 id) public view returns (uint256) {
        return uint256(jobs[id].deadline) + REVIEW;
    }

    function cancelJob(bytes32 id) external nonReentrant {
        T.Job storage j = jobs[id];
        if (
            (j.state != 1 && j.state != 2) || j.children != 0 || j.parent != 0
                || (msg.sender != j.executor && (msg.sender != j.buyer || block.timestamp <= reviewEnd(id)))
        ) revert Authority();
        uint256 amount = j.available;
        _cancel(j);
        _pay(j.buyer, amount);
        emit JobCancelled(id, amount);
    }

    function _cancel(T.Job storage j) internal {
        j.refunded = j.available;
        j.available = 0;
        j.state = 4;
        j.cancelledAt = uint64(block.timestamp);
    }

    function returnBranch(bytes32 id) external {
        T.Job storage j = jobs[id];
        T.Job storage p = jobs[j.parent];
        if (
            j.parent == 0 || (j.state != 1 && j.state != 2) || j.children != 0
                || (msg.sender != j.executor
                    && ((msg.sender != j.buyer && msg.sender != jobs[j.root].buyer)
                        || block.timestamp <= reviewEnd(id)))
        ) revert Authority();
        p.available += j.available;
        --p.children;
        uint256 amount = j.available;
        _cancel(j);
        emit JobCancelled(id, amount);
    }

    function closeBranch(bytes32 id) external {
        T.Job storage j = jobs[id];
        T.Job storage p = jobs[j.parent];
        if (
            j.parent == 0 || j.state != 3 || j.children != 0
                || (msg.sender != p.executor && msg.sender != jobs[j.root].buyer)
        ) revert Authority();
        --p.children;
        j.state = 5;
        emit BranchClosed(id);
    }
}
