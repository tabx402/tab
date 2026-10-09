// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {Test} from "forge-std/Test.sol";
import {StdInvariant} from "forge-std/StdInvariant.sol";
import {MockToken} from "./TabBNB.t.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabBacking} from "../src/TabBacking.sol";
import {TabEconomics} from "../src/TabEconomics.sol";
import {TabTypes as T} from "../src/TabTypes.sol";

contract BudgetHandler is Test {
    TabProtocol public p;
    TabBacking public b;
    MockToken public u;
    bytes32 public immutable A;
    bytes32 public constant CREDIT = keccak256("handler-credit");
    bytes32 public constant H = keccak256("hash");
    address public constant MERCHANT = address(0xCAFE);
    bytes32 public sid;
    bytes32[] public ids;
    uint256 public serial;

    constructor(TabProtocol p_, TabBacking b_, MockToken u_) {
        A = bytes32((uint256(uint160(address(this))) << 96) | 1);
        p = p_;
        b = b_;
        u = u_;
        u.mint(address(this), 1e36);
        u.approve(address(p), type(uint256).max);
        u.approve(address(b), type(uint256).max);
        p.register(A, "budget handler", 10_000 ether, H);
        address[] memory recipients = new address[](1);
        recipients[0] = MERCHANT;
        p.grantSession(
            A,
            T.SessionTerms(
                1,
                address(this),
                uint64(block.timestamp + 31 days),
                100 ether,
                10_000 ether,
                10_000 ether,
                7,
                recipients
            )
        );
        sid = p.sessionId(A, 1);
        b.openCredit(
            T.CreditTerms(
                CREDIT,
                A,
                address(this),
                1000 ether,
                100 ether,
                1000 ether,
                uint64(block.timestamp + 31 days),
                7,
                recipients
            )
        );
        b.pledgeCollateral(CREDIT, 2000 ether);
        b.acceptCredit(CREDIT);
    }

    function _providers() internal pure returns (address[] memory recipients) {
        recipients = new address[](1);
        recipients[0] = MERCHANT;
    }

    function fund(uint96 raw) external {
        p.fundSpending(A, bound(raw, 1, 100 ether));
    }

    function withdraw(uint96 raw) external {
        uint256 available = p.getSpending(A).available;
        if (available > 0) p.withdrawSpending(A, bound(raw, 1, available));
    }

    function sessionSpend(uint96 raw) external {
        T.Spending memory f = p.getSpending(A);
        T.Session memory s = p.getSession(sid);
        T.Agent memory a = p.getAgent(A);
        uint256 max = 100 ether;
        if (f.available < max) max = f.available;
        if (s.totalCap - s.totalSpent < max) max = s.totalCap - s.totalSpent;
        if (a.dailyCap - a.dailySpent < max) max = a.dailyCap - a.dailySpent;
        if (max > 0) p.sessionPay(sid, MERCHANT, bound(raw, 1, max), 1, bytes32(++serial), H);
    }

    function creditSpend(uint96 raw) external {
        T.Credit memory c = b.getCredit(CREDIT);
        T.Agent memory a = p.getAgent(A);
        uint256 max = 100 ether;
        if (c.available < max) max = c.available;
        if (c.dailyCap - c.dailySpent < max) max = c.dailyCap - c.dailySpent;
        if (a.dailyCap - a.dailySpent < max) max = a.dailyCap - a.dailySpent;
        if (max > 0) b.spendCredit(CREDIT, MERCHANT, bound(raw, 1, max), 1, bytes32(++serial), H);
    }

    function repay(uint96 raw) external {
        uint256 outstanding = b.getCredit(CREDIT).outstanding;
        if (outstanding > 0) b.repayCredit(CREDIT, bound(raw, 1, outstanding));
    }

    function withdrawCredit(uint96 raw) external {
        uint256 available = b.getCredit(CREDIT).available;
        if (available > 0) b.withdrawCredit(CREDIT, bound(raw, 1, available));
    }

    function openJob(uint96 raw) external {
        bytes32 id = keccak256(abi.encode(++serial));
        uint256 amount = bound(raw, 1 ether, 100 ether);
        p.openJob(
            T.JobTerms(
                id, address(this), amount, 1 ether, uint64(block.timestamp + 2 days), H, 7, _providers()
            )
        );
        p.bindJobAgent(id, A);
        ids.push(id);
    }

    function delegate(uint256 seed, uint96 raw) external {
        if (ids.length == 0) return;
        bytes32 parent = ids[seed % ids.length];
        T.Job memory j = p.getJob(parent);
        if (j.state != 1 || j.available < 1 ether || j.depth >= 8) return;
        bytes32 id = keccak256(abi.encode(++serial));
        uint256 amount = bound(raw, 1 ether, j.available);
        p.delegateJob(parent, T.JobTerms(id, address(this), amount, 1 ether, j.deadline, H, 7, _providers()));
        p.bindJobAgent(id, A);
        ids.push(id);
    }

    function cancel(uint256 seed) external {
        if (ids.length == 0) return;
        bytes32 id = ids[seed % ids.length];
        T.Job memory j = p.getJob(id);
        if (j.state != 1 || j.children != 0) return;
        if (j.parent == 0) p.cancelJob(id);
        else p.returnBranch(id);
    }

    function complete(uint256 seed) external {
        if (ids.length == 0) return;
        bytes32 id = ids[seed % ids.length];
        T.Job memory j = p.getJob(id);
        if (j.state != 1 || j.children != 0) return;
        p.submitJob(id, H);
        p.acceptJob(id, H);
        if (j.parent != 0) p.closeBranch(id);
    }

    function providerSpend(uint256 seed, uint96 raw) external {
        if (ids.length == 0) return;
        bytes32 id = ids[seed % ids.length];
        T.Job memory j = p.getJob(id);
        T.Agent memory a = p.getAgent(A);
        if (j.state != 1) return;
        uint256 max = j.available;
        if (max > j.maxCall) max = j.maxCall;
        if (a.dailyCap - a.dailySpent < max) max = a.dailyCap - a.dailySpent;
        if (max > 0) p.payCall(id, MERCHANT, bound(raw, 1, max), 1, bytes32(++serial), H);
    }

    function sumAvailableJobs() external view returns (uint256 sum) {
        for (uint256 i; i < ids.length; i++) {
            sum += p.getJob(ids[i]).available;
        }
    }
}

contract TabInvariantTest is StdInvariant, Test {
    TabProtocol p;
    TabBacking b;
    TabEconomics e;
    MockToken u;
    BudgetHandler h;

    function setUp() public {
        vm.chainId(31337);
        vm.warp(10 days);
        u = new MockToken(18);
        p = TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(u), address(this))));
        b = TabBacking(deployCode("TabBacking.sol:TabBacking", abi.encode(address(p))));
        e = TabEconomics(deployCode("TabEconomics.sol:TabEconomics", abi.encode(address(p))));
        p.configureModules(address(b), address(e));
        h = new BudgetHandler(p, b, u);
        targetContract(address(h));
    }

    function invariantProtocolSolvencyAndNoDoubleReservations() public view {
        uint256 accounted = p.getSpending(h.A()).available + p.feesCollectedUsdt() + h.sumAvailableJobs();
        assertEq(accounted, p.totalLiability());
        assertEq(u.balanceOf(address(p)), accounted);
    }

    function invariantLenderPrincipalAndTokenSolvency() public view {
        T.Credit memory c = b.getCredit(h.CREDIT());
        assertEq(c.funded, c.available + c.outstanding + c.withdrawn);
        assertEq(c.totalSpent - c.totalRepaid, c.outstanding);
        assertEq(u.balanceOf(address(b)), c.available + 2000 ether);
        assertEq(b.tokenLiability(address(u)), c.available + 2000 ether);
    }

    function invariantAgentCapsAreSharedByEverySpendingPath() public view {
        T.Agent memory a = p.getAgent(h.A());
        assertLe(a.dailySpent, a.dailyCap);
        T.Session memory s = p.getSession(h.sid());
        assertLe(s.totalSpent, s.totalCap);
        assertLe(s.dailySpent, s.dailyCap);
    }
}
