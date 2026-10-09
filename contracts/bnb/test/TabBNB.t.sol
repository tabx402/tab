// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {Test} from "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabBacking} from "../src/TabBacking.sol";
import {TabEconomics, TabAgentToken} from "../src/TabEconomics.sol";
import {TabTypes as T} from "../src/TabTypes.sol";

contract MockToken is ERC20 {
    uint8 private precision;

    constructor(uint8 d) ERC20("Mock token", "MOCK") {
        precision = d;
    }

    function decimals() public view override returns (uint8) {
        return precision;
    }

    function mint(address a, uint256 v) external {
        _mint(a, v);
    }
}

contract TaxToken is MockToken {
    constructor() MockToken(18) {}

    function _update(address a, address b, uint256 n) internal override {
        if (a != address(0) && b != address(0)) {
            super._update(a, address(0), n / 100);
            n -= n / 100;
        }
        super._update(a, b, n);
    }
}

contract ReenterBacker {
    TabBacking b;
    bytes32 a;
    bool public blocked;

    constructor(TabBacking b_, bytes32 a_) {
        b = b_;
        a = a_;
    }

    function deposit() external payable {
        b.backBNB{value: msg.value}(a);
    }

    function withdraw() external {
        b.withdrawBNB(a, 1 ether);
    }

    receive() external payable {
        try b.withdrawBNB(a, 1 ether) {}
        catch {
            blocked = true;
        }
    }
}

contract TabBase is Test {
    TabProtocol p;
    TabBacking b;
    TabEconomics e;
    MockToken u;
    MockToken tab;
    address alice = address(0xA11CE);
    address bob = address(0xB0B);
    address lender = address(0x1E);
    address merchant = address(0xCAFE);
    address signer = address(0x510);
    address carol = address(0xCA20);
    bytes32 constant A = bytes32((uint256(uint160(address(0xA11CE))) << 96) | 1);
    bytes32 constant B = bytes32((uint256(uint160(address(0xB0B))) << 96) | 1);
    bytes32 constant J = keccak256("job");
    bytes32 constant C = keccak256("child");
    bytes32 constant CREDIT = keccak256("credit");
    bytes32 constant POLICY = keccak256("policy");
    bytes32 constant EVIDENCE = keccak256("evidence");

    function setUp() public virtual {
        vm.chainId(31337);
        vm.warp(10 days);
        u = new MockToken(18);
        tab = new MockToken(18);
        p = TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(u), address(this))));
        b = TabBacking(deployCode("TabBacking.sol:TabBacking", abi.encode(address(p))));
        e = TabEconomics(deployCode("TabEconomics.sol:TabEconomics", abi.encode(address(p))));
        p.configureModules(address(b), address(e));
        address[5] memory actors = [alice, bob, lender, merchant, carol];
        for (uint256 i; i < actors.length; i++) {
            u.mint(actors[i], 1_000_000 ether);
            tab.mint(actors[i], 1_000 ether);
            vm.deal(actors[i], 10 ether);
            vm.startPrank(actors[i]);
            u.approve(address(p), type(uint256).max);
            u.approve(address(b), type(uint256).max);
            u.approve(address(e), type(uint256).max);
            tab.approve(address(e), type(uint256).max);
            vm.stopPrank();
        }
        vm.prank(alice);
        p.register(A, "wren", 100 ether, POLICY);
        vm.prank(bob);
        p.register(B, "finch", 100 ether, POLICY);
    }

    function _providers() internal view returns (address[] memory recipients) {
        recipients = new address[](1);
        recipients[0] = merchant;
    }

    function _terms(bytes32 id, address executor, uint256 amount) internal view returns (T.JobTerms memory) {
        return
            T.JobTerms(
                id, executor, amount, 10 ether, uint64(block.timestamp + 2 days), POLICY, 7, _providers()
            );
    }

    function _job() internal {
        vm.prank(bob);
        p.openJob(_terms(J, alice, 100 ether));
        vm.prank(alice);
        p.bindJobAgent(J, A);
    }

    function _session(uint256 cap) internal returns (bytes32 sid) {
        address[] memory recipients = new address[](1);
        recipients[0] = merchant;
        vm.prank(alice);
        p.grantSession(
            A,
            T.SessionTerms(
                1, signer, uint64(block.timestamp + 2 days), 10 ether, cap, 100 ether, 7, recipients
            )
        );
        sid = p.sessionId(A, 1);
        vm.prank(alice);
        p.fundSpending(A, 100 ether);
    }

    function _credit() internal {
        address[] memory recipients = new address[](1);
        recipients[0] = merchant;
        vm.prank(lender);
        b.openCredit(
            T.CreditTerms(
                CREDIT,
                A,
                signer,
                100 ether,
                10 ether,
                50 ether,
                uint64(block.timestamp + 2 days),
                7,
                recipients
            )
        );
        vm.prank(alice);
        b.pledgeCollateral(CREDIT, 200 ether);
    }

    function _coin() internal returns (TabAgentToken coin) {
        vm.prank(alice);
        coin = TabAgentToken(e.deployAgentToken(A, "Wren", "WREN", 1_000_000 ether, POLICY));
        vm.prank(alice);
        coin.approve(address(e), type(uint256).max);
    }

    function _economyJob() internal returns (TabAgentToken coin) {
        p.configureTab(address(tab));
        coin = _coin();
        _job();
    }
}

contract RegistryTest is TabBase {
    function testRegistrationAndOwnerPolicy() public {
        assertEq(p.getAgent(A).owner, alice);
        vm.prank(bob);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.updatePolicy(A, 5 ether, POLICY);
        vm.prank(alice);
        p.updatePolicy(A, 5 ether, EVIDENCE);
        assertEq(p.getAgent(A).version, 2);
        assertEq(p.getAgent(A).dailyCap, 5 ether);
        vm.prank(alice);
        p.pauseAgent(A, true);
        assertTrue(p.getAgent(A).paused);
    }

    function testInvalidRegistrationAndDuplicate() public {
        vm.expectRevert(TabProtocol.Terms.selector);
        p.register(A, "steal", 1 ether, POLICY);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.register(J, "x", 1 ether, POLICY);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.register(J, "okay", 0, POLICY);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.register(J, "okay", 1 ether, 0);
    }

    function testOwnerNamespacePreventsFrontRunningRegistration() public {
        bytes32 id = bytes32((uint256(uint160(alice)) << 96) | 99);
        vm.prank(bob);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.register(id, "squatter", 1 ether, POLICY);
        vm.prank(alice);
        p.register(id, "mine", 1 ether, POLICY);
        assertEq(p.getAgent(id).owner, alice);
    }

    function testProductionChainAndDecimalsAreEnforced() public {
        MockToken six = new MockToken(6);
        vm.expectRevert(TabProtocol.Terms.selector);
        new TabProtocol(address(six), address(this));
        vm.chainId(1);
        vm.expectRevert(TabProtocol.Terms.selector);
        new TabProtocol(address(u), address(this));
        vm.chainId(56);
        vm.expectRevert(TabProtocol.Terms.selector);
        new TabProtocol(address(u), address(this));
    }

    function testModuleAndOfficialTokenConfigurationAreOneTime() public {
        vm.expectRevert(TabProtocol.Authority.selector);
        p.configureModules(address(b), address(e));
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.configureTab(address(tab));
        p.configureTab(address(tab));
        vm.expectRevert(TabProtocol.Authority.selector);
        p.configureTab(address(tab));
        assertEq(p.tabToken(), address(tab));
    }

    function _agentId(address owner) internal pure returns (bytes32) {
        return bytes32((uint256(uint160(owner)) << 96) | 1);
    }

    function _signature(uint256 key, address target, uint256 chain, uint256 nonce, uint256 deadline)
        internal
        view
        returns (bytes memory)
    {
        bytes32 domain = keccak256(
            abi.encode(
                keccak256(
                    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                ),
                keccak256("Tab Protocol"),
                keccak256("1"),
                chain,
                target
            )
        );
        bytes32 message = keccak256(
            abi.encode(
                p.REGISTER_TYPEHASH(),
                _agentId(vm.addr(key)),
                vm.addr(key),
                keccak256("sponsored"),
                1 ether,
                POLICY,
                nonce,
                deadline
            )
        );
        (uint8 v, bytes32 r, bytes32 s) =
            vm.sign(key, keccak256(abi.encodePacked("\x19\x01", domain, message)));
        return abi.encodePacked(r, s, v);
    }

    function testSponsoredRegistrationBoundToOwnerNonceChainContractAndTerms() public {
        uint256 key = 12345;
        address owner = vm.addr(key);
        uint256 deadline = block.timestamp + 60;
        bytes memory sig = _signature(key, address(p), 31337, 0, deadline);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.registerWithSignature(_agentId(owner), owner, "changed", 1 ether, POLICY, 0, deadline, sig);
        vm.chainId(56);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.registerWithSignature(_agentId(owner), owner, "sponsored", 1 ether, POLICY, 0, deadline, sig);
        vm.chainId(31337);
        TabProtocol other = new TabProtocol(address(u), address(this));
        vm.expectRevert(TabProtocol.Authority.selector);
        other.registerWithSignature(_agentId(owner), owner, "sponsored", 1 ether, POLICY, 0, deadline, sig);
        p.registerWithSignature(_agentId(owner), owner, "sponsored", 1 ether, POLICY, 0, deadline, sig);
        assertEq(p.getAgent(_agentId(owner)).owner, owner);
        assertEq(p.registrationNonces(owner), 1);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.registerWithSignature(_agentId(owner), owner, "sponsored", 1 ether, POLICY, 0, deadline, sig);
    }

    function testExpiredRegistrationRejected() public {
        uint256 deadline = block.timestamp - 1;
        bytes memory sig = _signature(7, address(p), 31337, 0, deadline);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.registerWithSignature(
            _agentId(vm.addr(7)), vm.addr(7), "sponsored", 1 ether, POLICY, 0, deadline, sig
        );
    }
}

contract JobsTest is TabBase {
    function testRootJobConservesProviderRewardAndFee() public {
        _job();
        uint256 before_ = u.balanceOf(alice);
        vm.prank(alice);
        p.payCall(J, merchant, 10 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        p.submitJob(J, EVIDENCE);
        vm.prank(bob);
        p.acceptJob(J, EVIDENCE);
        T.Job memory j = p.getJob(J);
        assertEq(j.providerSpent, 10 ether);
        assertEq(j.rewardPaid, 89.55 ether);
        assertEq(j.feePaid, 0.45 ether);
        assertEq(u.balanceOf(alice) - before_, 89.55 ether);
        assertEq(u.balanceOf(address(p)), p.totalLiability());
        assertEq(p.feesCollectedUsdt(), 0.45 ether);
    }

    function testDelegationReservesBudgetAndReturnsExactlyOnce() public {
        _job();
        T.JobTerms memory t = _terms(C, carol, 40 ether);
        vm.prank(alice);
        p.delegateJob(J, t);
        assertEq(p.getJob(J).available, 60 ether);
        vm.prank(carol);
        p.returnBranch(C);
        assertEq(p.getJob(J).available, 100 ether);
        assertEq(p.getJob(J).children, 0);
        vm.prank(carol);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.returnBranch(C);
    }

    function testNestedPaidBranchesConserveRootVault() public {
        _job();
        vm.prank(alice);
        p.delegateJob(J, _terms(C, carol, 40 ether));
        bytes32 leaf = keccak256("leaf");
        vm.prank(carol);
        p.delegateJob(C, _terms(leaf, alice, 20 ether));
        vm.prank(alice);
        p.submitJob(leaf, EVIDENCE);
        vm.prank(bob);
        p.acceptJob(leaf, EVIDENCE);
        vm.prank(bob);
        p.closeBranch(leaf);
        vm.prank(carol);
        p.submitJob(C, EVIDENCE);
        vm.prank(bob);
        p.acceptJob(C, EVIDENCE);
        vm.prank(bob);
        p.closeBranch(C);
        vm.prank(alice);
        p.submitJob(J, EVIDENCE);
        vm.prank(bob);
        p.acceptJob(J, EVIDENCE);
        assertEq(u.balanceOf(address(p)), 0.5 ether);
        assertEq(p.totalLiability(), 0.5 ether);
        assertEq(p.completedWorkUsdt(), 100 ether);
    }

    function testParentCannotSelfApproveOrRejectDelegatedReward() public {
        _job();
        vm.prank(alice);
        p.delegateJob(J, _terms(C, alice, 40 ether));
        vm.prank(alice);
        p.submitJob(C, EVIDENCE);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.acceptJob(C, EVIDENCE);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.rejectJob(C);
        vm.prank(bob);
        p.rejectJob(C);
        vm.prank(alice);
        p.submitJob(C, EVIDENCE);
        vm.prank(bob);
        p.acceptJob(C, EVIDENCE);
        assertEq(p.getJob(C).rewardPaid, 39.8 ether);
        vm.prank(bob);
        p.closeBranch(C);
        assertEq(p.getJob(J).available, 60 ether);
    }

    function testCannotAcceptWhileChildrenOpen() public {
        _job();
        vm.prank(alice);
        p.delegateJob(J, _terms(C, carol, 40 ether));
        vm.prank(alice);
        p.submitJob(J, EVIDENCE);
        vm.prank(bob);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.acceptJob(J, EVIDENCE);
    }

    function testDelegationCannotExpandCapsDeadlineToolsOrBudget() public {
        _job();
        T.JobTerms memory t = _terms(C, carol, 40 ether);
        t.maxCall = 11 ether;
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.delegateJob(J, t);
        t.maxCall = 10 ether;
        t.tools = 8;
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.delegateJob(J, t);
        t.tools = 7;
        t.deadline += 1;
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.delegateJob(J, t);
        t.deadline -= 1;
        t.budget = 101 ether;
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.delegateJob(J, t);
    }

    function testDelegationDepthBound() public {
        _job();
        bytes32 parent = J;
        for (uint256 i = 1; i <= 8; i++) {
            bytes32 id = bytes32(i);
            vm.prank(alice);
            p.delegateJob(parent, _terms(id, alice, 10 ether));
            parent = id;
        }
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.delegateJob(parent, _terms(bytes32(uint256(9)), alice, 10 ether));
    }

    function testPauseStopsPaymentsButNeverTimelyEvidenceOrRefund() public {
        _job();
        vm.prank(bob);
        p.pauseJob(J, true);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.payCall(J, merchant, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        p.submitJob(J, EVIDENCE);
        assertTrue(p.getJob(J).timelySubmitted);
        vm.prank(bob);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.acceptJob(J, EVIDENCE);
        vm.warp(block.timestamp + 3 days + 1);
        vm.prank(bob);
        p.cancelJob(J);
        assertEq(p.getJob(J).refunded, 100 ether);
    }

    function testBuyerCannotCancelBeforeReviewPeriod() public {
        _job();
        vm.prank(bob);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.cancelJob(J);
        vm.warp(block.timestamp + 3 days + 1);
        vm.prank(bob);
        p.cancelJob(J);
        assertEq(u.balanceOf(address(p)), 0);
    }

    function testJobRecipientsBoundByBuyerAndInheritedByBranches() public {
        _job();
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.payCall(J, carol, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.payCall(J, alice, 1 ether, 1, POLICY, EVIDENCE);
        T.JobTerms memory t = _terms(C, carol, 40 ether);
        t.recipients[0] = lender;
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.delegateJob(J, t);
        t.recipients = new address[](0);
        vm.prank(alice);
        p.delegateJob(J, t);
        assertEq(p.getJob(C).recipients.length, 0);
    }

    function testJobWithoutProvidersCannotPayCalls() public {
        T.JobTerms memory t = _terms(J, alice, 100 ether);
        t.recipients = new address[](0);
        vm.prank(bob);
        p.openJob(t);
        vm.prank(alice);
        p.bindJobAgent(J, A);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.payCall(J, merchant, 1 ether, 1, POLICY, EVIDENCE);
    }

    function testPayCallRequiresBoundOwnerToolsCapAndUniqueRequest() public {
        _job();
        vm.prank(carol);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.payCall(J, merchant, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.payCall(J, merchant, 1 ether, 3, POLICY, EVIDENCE);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.payCall(J, merchant, 11 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        p.payCall(J, merchant, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.payCall(J, merchant, 1 ether, 1, POLICY, EVIDENCE);
    }

    function testFuzzBranchConservation(uint96 raw, uint96 childRaw) public {
        uint256 total = bound(raw, 10 ether, 10_000 ether);
        uint256 child = bound(childRaw, 10 ether, total);
        T.JobTerms memory t = _terms(J, alice, total);
        vm.prank(bob);
        p.openJob(t);
        vm.prank(alice);
        p.delegateJob(J, _terms(C, carol, child));
        assertEq(p.getJob(J).available + p.getJob(C).available, total);
        vm.prank(carol);
        p.returnBranch(C);
        vm.prank(alice);
        p.cancelJob(J);
        assertEq(u.balanceOf(address(p)), 0);
        assertEq(p.totalLiability(), 0);
    }
}

contract SessionsTest is TabBase {
    function testSessionRecipientReplayCapsAndRevocation() public {
        bytes32 sid = _session(10 ether);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.sessionPay(sid, carol, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(signer);
        p.sessionPay(sid, merchant, 6 ether, 1, POLICY, EVIDENCE);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.sessionPay(sid, merchant, 6 ether, 1, POLICY, EVIDENCE);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.DailyCap.selector);
        p.sessionPay(sid, merchant, 5 ether, 1, EVIDENCE, POLICY);
        vm.prank(alice);
        p.revokeSession(sid);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.sessionPay(sid, merchant, 1 ether, 1, EVIDENCE, POLICY);
    }

    function testSessionAgentDailyCapAcrossSessionsAndDays() public {
        bytes32 sid = _session(100 ether);
        vm.prank(alice);
        p.updatePolicy(A, 5 ether, POLICY);
        vm.prank(signer);
        p.sessionPay(sid, merchant, 5 ether, 1, POLICY, EVIDENCE);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.DailyCap.selector);
        p.sessionPay(sid, merchant, 1 ether, 1, EVIDENCE, POLICY);
        vm.warp(block.timestamp + 1 days);
        vm.prank(signer);
        p.sessionPay(sid, merchant, 5 ether, 1, EVIDENCE, POLICY);
        assertEq(p.getAgent(A).dailySpent, 5 ether);
    }

    function testAgentWithdrawalCannotOverdrawAndSessionCannotSpendWithdrawnFunds() public {
        bytes32 sid = _session(100 ether);
        vm.prank(bob);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.withdrawSpending(A, 100 ether);
        vm.prank(alice);
        p.withdrawSpending(A, 100 ether);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.Terms.selector);
        p.sessionPay(sid, merchant, 1 ether, 1, POLICY, EVIDENCE);
        assertEq(p.getSpending(A).available, 0);
        assertEq(p.totalLiability(), 0);
    }

    function testExpiredSessionAndPausedAgentFail() public {
        bytes32 sid = _session(100 ether);
        vm.prank(alice);
        p.pauseAgent(A, true);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.sessionPay(sid, merchant, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        p.pauseAgent(A, false);
        vm.warp(block.timestamp + 2 days);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.sessionPay(sid, merchant, 1 ether, 1, POLICY, EVIDENCE);
    }
}

contract BackingTest is TabBase {
    function testExactStockDecimalsAndCustody() public {
        MockToken stock = new MockToken(8);
        stock.mint(alice, 125000000);
        vm.startPrank(alice);
        stock.approve(address(b), 125000000);
        b.backAgent(A, address(stock), 125000000);
        assertEq(b.getBacking(A, alice, address(stock)).decimals, 8);
        b.withdrawBacking(A, address(stock), 125000000);
        vm.stopPrank();
        assertEq(stock.balanceOf(alice), 125000000);
        assertEq(b.tokenLiability(address(stock)), 0);
    }

    function testTaxTokenCannotCorruptCustody() public {
        TaxToken stock = new TaxToken();
        stock.mint(alice, 100 ether);
        vm.startPrank(alice);
        stock.approve(address(b), 100 ether);
        vm.expectRevert(TabBacking.TransferFailed.selector);
        b.backAgent(A, address(stock), 100 ether);
        vm.stopPrank();
        assertEq(b.getBacking(A, alice, address(stock)).amount, 0);
    }

    function testNativeBackingExactOwnershipAndReentrancy() public {
        ReenterBacker r = new ReenterBacker(b, A);
        r.deposit{value: 1 ether}();
        r.withdraw();
        assertTrue(r.blocked());
        assertEq(address(b).balance, 0);
        assertEq(b.nativeLiability(), 0);
    }

    function testCreditMustBeAcceptedAndOnlyAllowlistedPayments() public {
        _credit();
        vm.prank(signer);
        vm.expectRevert(TabBacking.Terms.selector);
        b.spendCredit(CREDIT, merchant, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(alice);
        b.acceptCredit(CREDIT);
        vm.prank(signer);
        vm.expectRevert(TabBacking.Authority.selector);
        b.spendCredit(CREDIT, carol, 1 ether, 1, POLICY, EVIDENCE);
        vm.prank(signer);
        b.spendCredit(CREDIT, merchant, 10 ether, 1, POLICY, EVIDENCE);
        T.Credit memory c = b.getCredit(CREDIT);
        assertEq(c.available, 90 ether);
        assertEq(c.outstanding, 10 ether);
        vm.prank(signer);
        vm.expectRevert(TabBacking.Terms.selector);
        b.spendCredit(CREDIT, merchant, 10 ether, 1, POLICY, EVIDENCE);
    }

    function testLenderWithdrawsOnlyUnspentAndRepaidPrincipal() public {
        _credit();
        vm.prank(alice);
        b.acceptCredit(CREDIT);
        vm.prank(alice);
        b.spendCredit(CREDIT, merchant, 10 ether, 1, POLICY, EVIDENCE);
        vm.prank(lender);
        vm.expectRevert(TabBacking.Terms.selector);
        b.withdrawCredit(CREDIT, 100 ether);
        vm.prank(lender);
        b.closeCredit(CREDIT);
        vm.prank(lender);
        b.withdrawCredit(CREDIT, 90 ether);
        vm.prank(alice);
        b.repayCredit(CREDIT, 10 ether);
        vm.prank(lender);
        b.withdrawCredit(CREDIT, 10 ether);
        T.Credit memory c = b.getCredit(CREDIT);
        assertEq(c.outstanding, 0);
        assertEq(c.withdrawn, 100 ether);
        assertEq(c.available, 0);
        assertEq(u.balanceOf(address(b)), 200 ether);
    }

    function testCreditAndSessionsShareAgentDailyLimit() public {
        bytes32 sid = _session(100 ether);
        _credit();
        vm.prank(alice);
        b.acceptCredit(CREDIT);
        vm.prank(alice);
        p.updatePolicy(A, 10 ether, POLICY);
        vm.prank(signer);
        p.sessionPay(sid, merchant, 6 ether, 1, POLICY, EVIDENCE);
        vm.prank(signer);
        vm.expectRevert(TabProtocol.DailyCap.selector);
        b.spendCredit(CREDIT, merchant, 5 ether, 1, POLICY, EVIDENCE);
        assertEq(b.getCredit(CREDIT).outstanding, 0);
        vm.prank(signer);
        b.spendCredit(CREDIT, merchant, 4 ether, 1, POLICY, EVIDENCE);
        assertEq(p.getAgent(A).dailySpent, 10 ether);
    }

    function testCreditDebtCannotBeWithdrawnByBorrowerOrThirdParty() public {
        _credit();
        vm.prank(alice);
        b.acceptCredit(CREDIT);
        vm.prank(alice);
        vm.expectRevert(TabBacking.Authority.selector);
        b.withdrawCredit(CREDIT, 1 ether);
        vm.prank(carol);
        vm.expectRevert(TabBacking.Terms.selector);
        b.spendCredit(CREDIT, merchant, 1 ether, 1, POLICY, EVIDENCE);
    }

    function testClosedOrExpiredCreditCannotSpendButRepaymentWorks() public {
        _credit();
        vm.prank(alice);
        b.acceptCredit(CREDIT);
        vm.prank(alice);
        b.spendCredit(CREDIT, merchant, 10 ether, 1, POLICY, EVIDENCE);
        vm.warp(block.timestamp + 2 days);
        vm.prank(alice);
        vm.expectRevert(TabBacking.Terms.selector);
        b.spendCredit(CREDIT, merchant, 1 ether, 1, EVIDENCE, POLICY);
        vm.prank(alice);
        b.repayCredit(CREDIT, 10 ether);
        assertEq(b.getCredit(CREDIT).available, 100 ether);
    }

    function testFuzzLendingPrincipalConservation(uint96 raw) public {
        _credit();
        vm.prank(alice);
        b.acceptCredit(CREDIT);
        uint256 amount = bound(raw, 1, 10 ether);
        vm.prank(alice);
        b.spendCredit(CREDIT, merchant, amount, 1, POLICY, EVIDENCE);
        T.Credit memory c = b.getCredit(CREDIT);
        assertEq(c.funded, c.available + c.outstanding + c.withdrawn);
        vm.prank(alice);
        b.repayCredit(CREDIT, amount);
        vm.prank(lender);
        b.withdrawCredit(CREDIT, 100 ether);
        assertEq(u.balanceOf(address(b)), 200 ether);
    }
}

contract EconomicsTest is TabBase {
    function testAgentTokenFixedSupplyPairingOnceAndOwner() public {
        TabAgentToken coin = _coin();
        assertEq(coin.totalSupply(), 1_000_000 ether);
        assertEq(coin.balanceOf(alice), 1_000_000 ether);
        assertTrue(e.getLaunch(A).factoryToken);
        vm.prank(alice);
        vm.expectRevert(TabEconomics.Terms.selector);
        e.deployAgentToken(A, "Again", "AGAIN", 1 ether, POLICY);
        vm.prank(bob);
        vm.expectRevert(TabEconomics.Authority.selector);
        e.pairAgentToken(A, address(tab), POLICY);
    }

    function testStakingRequiresActualOfficialTokenAndLock() public {
        vm.prank(alice);
        vm.expectRevert(TabEconomics.Terms.selector);
        e.stakeTab(1 ether, 1 days);
        p.configureTab(address(tab));
        vm.prank(alice);
        e.stakeTab(10 ether, 2 days);
        vm.prank(alice);
        vm.expectRevert(TabEconomics.Locked.selector);
        e.unstakeTab();
        vm.warp(block.timestamp + 2 days);
        vm.prank(alice);
        e.unstakeTab();
        assertEq(tab.balanceOf(alice), 1_000 ether);
    }

    function testBondTimelyRejectedEvidenceCannotBeSlashed() public {
        TabAgentToken coin = _economyJob();
        vm.prank(alice);
        e.bondJob(J, 10 ether, 5000);
        vm.prank(alice);
        p.submitJob(J, EVIDENCE);
        vm.prank(bob);
        p.rejectJob(J);
        vm.warp(block.timestamp + 3 days + 1);
        vm.prank(bob);
        p.cancelJob(J);
        e.settleBond(J);
        assertEq(e.getBond(J).penaltyPaid, 0);
        assertEq(coin.balanceOf(alice), 1_000_000 ether);
    }

    function testMissedDeadlineBondPenaltyAndDoubleSettlementPrevention() public {
        TabAgentToken coin = _economyJob();
        vm.prank(alice);
        e.bondJob(J, 10 ether, 2500);
        vm.prank(alice);
        p.cancelJob(J);
        vm.expectRevert(TabEconomics.Locked.selector);
        e.settleBond(J);
        vm.warp(block.timestamp + 3 days + 1);
        e.settleBond(J);
        assertEq(coin.balanceOf(bob), 2.5 ether);
        assertEq(e.getBond(J).returned, 7.5 ether);
        vm.expectRevert(TabEconomics.Terms.selector);
        e.settleBond(J);
    }

    function testBountyRequiresExactPairedAgentAndActualHolders() public {
        p.configureTab(address(tab));
        _coin();
        vm.prank(bob);
        p.openJob(_terms(J, address(0), 100 ether));
        vm.prank(carol);
        vm.expectRevert(TabProtocol.Authority.selector);
        p.claimJob(J, A);
        vm.prank(alice);
        p.claimJob(J, A);
        assertEq(p.getJob(J).executor, alice);
        assertEq(p.getJob(J).agent, A);
    }

    function _outcome() internal returns (TabAgentToken coin) {
        coin = _economyJob();
        vm.startPrank(alice);
        coin.transfer(bob, 1 ether);
        coin.transfer(carol, 1 ether);
        e.openOutcome(J, uint64(block.timestamp + 1 days));
        vm.stopPrank();
    }

    function testOutcomePayoutsConserveRoundingDustAndRejectDuplicates() public {
        _outcome();
        vm.prank(alice);
        e.betOutcome(J, 3, true);
        vm.prank(bob);
        e.betOutcome(J, 7, true);
        vm.prank(carol);
        e.betOutcome(J, 3, false);
        vm.prank(alice);
        p.submitJob(J, EVIDENCE);
        vm.warp(block.timestamp + 2 days + 1);
        e.resolveOutcome(J);
        vm.prank(alice);
        e.claimOutcome(J);
        vm.prank(bob);
        e.claimOutcome(J);
        vm.prank(carol);
        e.claimOutcome(J);
        assertEq(e.getPosition(J, alice).payout + e.getPosition(J, bob).payout, 13);
        assertEq(e.getOutcome(J).paidUsdt, 13);
        assertEq(u.balanceOf(address(e)), 0);
        vm.prank(alice);
        vm.expectRevert(TabEconomics.Terms.selector);
        e.claimOutcome(J);
    }

    function testSingleSidedOutcomeRefundsRatherThanInventingProfit() public {
        _outcome();
        vm.prank(alice);
        e.betOutcome(J, 10 ether, true);
        vm.warp(block.timestamp + 2 days + 1);
        e.resolveOutcome(J);
        assertEq(e.getOutcome(J).state, 4);
        vm.prank(alice);
        e.claimOutcome(J);
        assertEq(e.getPosition(J, alice).payout, 10 ether);
    }

    function testEarlyCancelledOutcomeRefundsBothSides() public {
        _outcome();
        vm.prank(alice);
        e.betOutcome(J, 10 ether, true);
        vm.prank(bob);
        e.betOutcome(J, 5 ether, false);
        vm.prank(alice);
        p.cancelJob(J);
        vm.warp(block.timestamp + 2 days + 1);
        e.resolveOutcome(J);
        assertEq(e.getOutcome(J).state, 4);
        vm.prank(alice);
        e.claimOutcome(J);
        vm.prank(bob);
        e.claimOutcome(J);
        assertEq(u.balanceOf(address(e)), 0);
    }

    function testNoBettingAfterSubmissionEvenIfBuyerRejectedIt() public {
        _outcome();
        vm.prank(alice);
        p.submitJob(J, EVIDENCE);
        vm.prank(bob);
        p.rejectJob(J);
        vm.prank(bob);
        vm.expectRevert(TabEconomics.Terms.selector);
        e.betOutcome(J, 1 ether, false);
    }

    function testFuzzOutcomeConservation(uint96 yesRaw, uint96 noRaw) public {
        _outcome();
        uint256 yes = bound(yesRaw, 1, 10_000 ether);
        uint256 no = bound(noRaw, 1, 10_000 ether);
        vm.prank(alice);
        e.betOutcome(J, yes, true);
        vm.prank(bob);
        e.betOutcome(J, no, false);
        vm.warp(block.timestamp + 2 days + 1);
        e.resolveOutcome(J);
        vm.prank(bob);
        e.claimOutcome(J);
        vm.prank(alice);
        e.claimOutcome(J);
        assertEq(e.getPosition(J, bob).payout, yes + no);
        assertEq(u.balanceOf(address(e)), 0);
    }
}
