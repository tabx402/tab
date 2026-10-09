// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {TabProtocol} from "./TabProtocol.sol";
import {TabTypes as T} from "./TabTypes.sol";

/// @notice Fixed supply, no owner, tax, pause, blacklist or further minting.
contract TabAgentToken is ERC20 {
    constructor(string memory name_, string memory symbol_, uint256 supply, address recipient)
        ERC20(name_, symbol_)
    {
        _mint(recipient, supply);
    }
}

/// @notice Agent coins, exact token bonds, time-locked TAB custody and objective USDT outcomes.
contract TabEconomics is ReentrancyGuard {
    using SafeERC20 for IERC20;
    TabProtocol public immutable protocol;
    IERC20 public immutable usdt;
    mapping(bytes32 => T.Launch) private launches;
    mapping(bytes32 => T.Bond) private bonds;
    mapping(address => T.Stake) private stakes;
    mapping(bytes32 => T.Outcome) private outcomes;
    mapping(bytes32 => mapping(address => T.Position)) private positions;
    mapping(address => uint256) public tokenLiability;
    error Terms();
    error Authority();
    error Locked();
    error TransferFailed();
    event TokenPaired(
        bytes32 indexed agent,
        address indexed token,
        address indexed creator,
        bytes32 metadataHash,
        bool factoryToken
    );
    event BondOpened(
        bytes32 indexed job, bytes32 indexed agent, address token, uint256 amount, uint16 penaltyBps
    );
    event BondSettled(
        bytes32 indexed job,
        address indexed token,
        uint256 penaltyUnits,
        uint256 returnedUnits,
        bool missedDeadline
    );
    event TabStaked(address indexed owner, uint256 amount, uint64 unlockAt);
    event TabUnstaked(address indexed owner, uint256 amount);
    event OutcomeOpened(bytes32 indexed job, bytes32 indexed agent, uint64 closesAt, uint64 deadline);
    event OutcomeBet(bytes32 indexed job, address indexed bettor, uint256 amount, bool side);
    event OutcomeResolved(bytes32 indexed job, uint8 state, uint256 forUsdt, uint256 againstUsdt);
    event OutcomeClaimed(bytes32 indexed job, address indexed bettor, uint256 payout);

    constructor(address protocol_) {
        protocol = TabProtocol(protocol_);
        usdt = protocol.usdt();
    }

    function getLaunch(bytes32 agent) external view returns (T.Launch memory) {
        return launches[agent];
    }

    function pairedToken(bytes32 agent) external view returns (address) {
        return launches[agent].token;
    }

    function getBond(bytes32 job) external view returns (T.Bond memory) {
        return bonds[job];
    }

    function getStake(address owner) external view returns (T.Stake memory) {
        return stakes[owner];
    }

    function getOutcome(bytes32 job) external view returns (T.Outcome memory) {
        return outcomes[job];
    }

    function getPosition(bytes32 job, address bettor) external view returns (T.Position memory) {
        return positions[job][bettor];
    }

    function _own(bytes32 agent) internal view {
        if (protocol.getAgent(agent).owner != msg.sender) revert Authority();
    }

    function _pull(IERC20 token, address from, uint256 amount) internal {
        uint256 before_ = token.balanceOf(address(this));
        token.safeTransferFrom(from, address(this), amount);
        if (token.balanceOf(address(this)) - before_ != amount) revert TransferFailed();
        tokenLiability[address(token)] += amount;
    }

    function _pay(IERC20 token, address to, uint256 amount) internal {
        if (amount == 0) return;
        uint256 before_ = token.balanceOf(to);
        tokenLiability[address(token)] -= amount;
        token.safeTransfer(to, amount);
        if (token.balanceOf(to) - before_ != amount) revert TransferFailed();
    }

    function _pair(bytes32 agent, address token, bytes32 metadataHash, bool factory) internal {
        if (
            launches[agent].token != address(0) || metadataHash == 0 || token.code.length == 0
                || token == address(usdt) || token == protocol.tabToken()
                || IERC20Metadata(token).decimals() > 18
        ) revert Terms();
        launches[agent] = T.Launch(agent, msg.sender, token, metadataHash, uint64(block.timestamp), factory);
        emit TokenPaired(agent, token, msg.sender, metadataHash, factory);
    }

    /// @dev Existing token pairing records identity only. The token issuer's mint/pause powers are not certified.
    function pairAgentToken(bytes32 agent, address token, bytes32 metadataHash) external {
        _own(agent);
        if (IERC20(token).balanceOf(msg.sender) == 0) revert Authority();
        _pair(agent, token, metadataHash, false);
    }

    function deployAgentToken(
        bytes32 agent,
        string calldata name,
        string calldata symbol,
        uint256 supply,
        bytes32 metadataHash
    ) external nonReentrant returns (address token) {
        _own(agent);
        if (
            bytes(name).length < 2 || bytes(name).length > 48 || bytes(symbol).length == 0
                || bytes(symbol).length > 12 || supply == 0 || supply > 1_000_000_000_000 ether
                || launches[agent].token != address(0)
        ) revert Terms();
        token = address(new TabAgentToken{salt: agent}(name, symbol, supply, msg.sender));
        _pair(agent, token, metadataHash, true);
    }

    function stakeTab(uint256 amount, uint64 lockSeconds) external nonReentrant {
        address token = protocol.tabToken();
        if (token == address(0) || amount == 0 || lockSeconds < 1 days || lockSeconds > 31 days) {
            revert Terms();
        }
        T.Stake storage s = stakes[msg.sender];
        uint64 unlockAt = uint64(block.timestamp) + lockSeconds;
        if (s.unlockAt > unlockAt) unlockAt = s.unlockAt;
        s.amount += amount;
        s.unlockAt = unlockAt;
        _pull(IERC20(token), msg.sender, amount);
        emit TabStaked(msg.sender, amount, unlockAt);
    }

    function unstakeTab() external nonReentrant {
        T.Stake storage s = stakes[msg.sender];
        if (s.amount == 0 || block.timestamp < s.unlockAt) revert Locked();
        uint256 amount = s.amount;
        delete stakes[msg.sender];
        _pay(IERC20(protocol.tabToken()), msg.sender, amount);
        emit TabUnstaked(msg.sender, amount);
    }

    function bondJob(bytes32 job, uint256 amount, uint16 penaltyBps) external nonReentrant {
        T.Job memory j = protocol.getJob(job);
        address token = launches[j.agent].token;
        if (
            j.executor != msg.sender || protocol.getAgent(j.agent).owner != msg.sender || j.state != 1
                || j.timelySubmitted || j.available == 0 || block.timestamp >= j.deadline || amount == 0
                || penaltyBps == 0 || penaltyBps > 10_000 || token == address(0)
                || bonds[job].owner != address(0)
        ) revert Terms();
        bonds[job] = T.Bond(j.agent, msg.sender, token, j.buyer, amount, penaltyBps, 0, 0, 0);
        _pull(IERC20(token), msg.sender, amount);
        emit BondOpened(job, j.agent, token, amount, penaltyBps);
    }

    function settleBond(bytes32 job) external nonReentrant {
        T.Job memory j = protocol.getJob(job);
        T.Bond storage b = bonds[job];
        if (b.owner == address(0) || b.state != 0 || (j.state != 3 && j.state != 4 && j.state != 5)) {
            revert Terms();
        }
        bool missed = !j.timelySubmitted && j.state == 4;
        if (missed && block.timestamp <= protocol.reviewEnd(job)) revert Locked();
        uint256 penalty = missed ? Math.mulDiv(b.amount, b.penaltyBps, 10_000) : 0;
        uint256 returned = b.amount - penalty;
        b.penaltyPaid = penalty;
        b.returned = returned;
        b.state = missed ? 2 : 1;
        _pay(IERC20(b.token), b.beneficiary, penalty);
        _pay(IERC20(b.token), b.owner, returned);
        emit BondSettled(job, b.token, penalty, returned, missed);
    }

    /// @dev Predicate: timely onchain evidence hash, never a prediction of quality or customer acceptance.
    function openOutcome(bytes32 job, uint64 closesAt) external {
        T.Job memory j = protocol.getJob(job);
        T.Launch memory l = launches[j.agent];
        if (
            j.state != 1 || j.timelySubmitted || j.executor != msg.sender
                || protocol.getAgent(j.agent).owner != msg.sender || closesAt <= block.timestamp
                || uint256(closesAt) + 60 > j.deadline || l.token == address(0)
                || protocol.tabToken() == address(0) || outcomes[job].state != 0
        ) revert Terms();
        T.Outcome storage o = outcomes[job];
        o.agent = j.agent;
        o.pairedToken = l.token;
        o.closesAt = closesAt;
        o.deadline = j.deadline;
        o.state = 1;
        emit OutcomeOpened(job, j.agent, closesAt, j.deadline);
    }

    function betOutcome(bytes32 job, uint256 amount, bool side) external nonReentrant {
        T.Outcome storage o = outcomes[job];
        T.Job memory j = protocol.getJob(job);
        if (
            o.state != 1 || block.timestamp >= o.closesAt || amount == 0 || amount > protocol.MAX_BUDGET()
                || j.state != 1 || j.timelySubmitted || positions[job][msg.sender].amount != 0
                || IERC20(protocol.tabToken()).balanceOf(msg.sender) == 0
                || IERC20(o.pairedToken).balanceOf(msg.sender) == 0
        ) revert Terms();
        if (side) {
            o.forUsdt += amount;
            ++o.forCount;
        } else {
            o.againstUsdt += amount;
            ++o.againstCount;
        }
        positions[job][msg.sender] = T.Position(amount, side, false, 0);
        _pull(usdt, msg.sender, amount);
        emit OutcomeBet(job, msg.sender, amount, side);
    }

    function resolveOutcome(bytes32 job) external {
        T.Outcome storage o = outcomes[job];
        T.Job memory j = protocol.getJob(job);
        if (o.state != 1 || block.timestamp <= o.deadline || j.deadline != o.deadline) revert Terms();
        if (
            o.forUsdt == 0 || o.againstUsdt == 0
                || (j.cancelledAt > 0 && j.cancelledAt <= o.closesAt && !j.timelySubmitted)
        ) o.state = 4;
        else o.state = j.timelySubmitted ? 2 : 3;
        emit OutcomeResolved(job, o.state, o.forUsdt, o.againstUsdt);
    }

    function claimOutcome(bytes32 job) external nonReentrant {
        T.Outcome storage o = outcomes[job];
        T.Position storage p = positions[job][msg.sender];
        if (p.amount == 0 || p.claimed || o.state < 2 || o.state > 4) revert Terms();
        uint256 total = o.forUsdt + o.againstUsdt;
        uint256 payout;
        if (o.state == 4) {
            payout = p.amount;
        } else if ((o.state == 2 && p.side) || (o.state == 3 && !p.side)) {
            uint256 winning = o.state == 2 ? o.forUsdt : o.againstUsdt;
            uint32 count = o.state == 2 ? o.forCount : o.againstCount;
            ++o.winningClaims;
            payout = o.winningClaims == count ? total - o.paidUsdt : Math.mulDiv(p.amount, total, winning);
        }
        p.claimed = true;
        p.payout = payout;
        o.paidUsdt += payout;
        if (o.paidUsdt > total) revert Terms();
        _pay(usdt, msg.sender, payout);
        emit OutcomeClaimed(job, msg.sender, payout);
    }
}
