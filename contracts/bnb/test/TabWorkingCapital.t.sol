// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {FinanceBase, FinanceToken, FinanceTaxToken, FinanceCallbackToken} from "./TabFinanceTestBase.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabLendingPool} from "../src/TabLendingPool.sol";
import {TabUSDTLiquidity} from "../src/TabUSDTLiquidity.sol";

contract TabWorkingCapitalTest is FinanceBase {
    TabLendingPool pool;

    function setUp() public override {
        super.setUp();
        pool = new TabLendingPool(address(protocol));
        _approve(address(usdt), lender, address(pool));
        _approve(address(usdt), borrower, address(pool));
        vm.prank(lender);
        pool.deposit(1_000 ether, lender);
        _job();
    }

    function _terms() internal view returns (TabLendingPool.LoanTerms memory t) {
        address[] memory recipients = new address[](1);
        recipients[0] = merchant;
        t = TabLendingPool.LoanTerms(
            LOAN,
            AGENT,
            JOB,
            signer,
            40 ether,
            10 ether,
            30 ether,
            uint64(block.timestamp + 1 days),
            3,
            recipients
        );
    }

    function _line() internal {
        pool.approveLoan(_terms());
        vm.prank(borrower);
        pool.acceptLoan(LOAN);
    }

    function _spend(uint256 amount, bytes32 request) internal {
        vm.prank(signer);
        pool.spendLoan(LOAN, merchant, amount, 1, request, RECEIPT);
    }

    function testDepositRedeemExactPrincipalAndNoYield() public {
        assertEq(pool.balanceOf(lender), 1_000 ether);
        assertEq(pool.totalAssets(), 1_000 ether);
        vm.prank(lender);
        assertEq(pool.redeem(1_000 ether, lender, lender), 1_000 ether);
        assertEq(pool.totalAssets(), 0);
        assertEq(usdt.balanceOf(address(pool)), 0);
    }

    function testLoanReservesPreventOvercommittedWithdrawals() public {
        _line();
        assertEq(pool.reserved(), 40 ether);
        assertEq(pool.maxWithdraw(lender), 960 ether);
        vm.prank(lender);
        vm.expectRevert();
        pool.withdraw(961 ether, lender, lender);
        _spend(10 ether, keccak256("request"));
        assertEq(pool.reserved(), 30 ether);
        assertEq(pool.outstanding(), 10 ether);
        assertEq(pool.totalAssets(), 1_000 ether);
        assertEq(pool.maxWithdraw(lender), 960 ether);
    }

    function testSpendReceiptAndExplicitRepayment() public {
        _line();
        uint256 beforeMerchant = usdt.balanceOf(merchant);
        _spend(7 ether, keccak256("paid"));
        assertEq(usdt.balanceOf(merchant) - beforeMerchant, 7 ether);
        assertEq(pool.getReceipt(LOAN, keccak256("paid")).receiptHash, RECEIPT);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 7 ether);
        assertEq(pool.outstanding(), 0);
        assertEq(pool.getLoan(LOAN).available, 33 ether); // Repayment is not a revolving line.
        pool.closeLoan(LOAN);
        uint256 shares = pool.balanceOf(lender);
        vm.prank(lender);
        pool.redeem(shares, lender, lender);
        assertEq(usdt.balanceOf(address(pool)), 0);
    }

    function testMustAcceptAndOnlySignerOrBorrowerCanSpend() public {
        pool.approveLoan(_terms());
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("before"), RECEIPT);
        vm.prank(borrower);
        pool.acceptLoan(LOAN);
        vm.prank(lender);
        vm.expectRevert(TabUSDTLiquidity.Authority.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("other"), RECEIPT);
    }

    function testOnlyUnderwriterApproves() public {
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Authority.selector);
        pool.approveLoan(_terms());
    }

    function testLoanRequiresFundedJobBoundToAgent() public {
        TabLendingPool.LoanTerms memory t = _terms();
        t.job = keccak256("not-a-job");
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.approveLoan(t);
    }

    function testRecipientsAndToolsAreSubsetOfJob() public {
        TabLendingPool.LoanTerms memory t = _terms();
        t.recipients[0] = lender;
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.approveLoan(t);
        t = _terms();
        t.tools = 8;
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.approveLoan(t);
    }

    function testSingleLinePerJobPreventsRepeatedAllocation() public {
        _line();
        TabLendingPool.LoanTerms memory t = _terms();
        t.id = keccak256("other-line");
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.approveLoan(t);
    }

    function testReplayToolAndPerCallLimits() public {
        _line();
        _spend(10 ether, keccak256("same"));
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("same"), RECEIPT);
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 3, keccak256("multi-bit"), RECEIPT);
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 11 ether, 1, keccak256("large"), RECEIPT);
    }

    function testDailyCapRollsAtDayBoundary() public {
        _line();
        for (uint256 i; i < 3; i++) {
            _spend(10 ether, bytes32(i + 1));
        }
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, bytes32(uint256(4)), RECEIPT);
        vm.warp(block.timestamp + 1 days);
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector); // Line expires at the next boundary.
        pool.spendLoan(LOAN, merchant, 1 ether, 1, bytes32(uint256(4)), RECEIPT);
    }

    function testPauseAllowsRepaymentCloseAndWithdraw() public {
        _line();
        _spend(10 ether, keccak256("paid"));
        pool.setPaused(true);
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("paused"), RECEIPT);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 10 ether);
        vm.prank(borrower);
        pool.closeLoan(LOAN);
        uint256 shares = pool.balanceOf(lender);
        vm.prank(lender);
        pool.redeem(shares, lender, lender);
        assertEq(pool.totalAssets(), 0);
    }

    function testJobOrAgentPauseStopsSpending() public {
        _line();
        vm.prank(buyer);
        protocol.pauseJob(JOB, true);
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("paused-job"), RECEIPT);
        vm.prank(buyer);
        protocol.pauseJob(JOB, false);
        vm.prank(borrower);
        protocol.pauseAgent(AGENT, true);
        vm.prank(signer);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("paused-agent"), RECEIPT);
    }

    function testAnyWalletReleasesExpiredReservations() public {
        _line();
        vm.warp(block.timestamp + 1 days);
        vm.prank(merchant);
        pool.closeLoan(LOAN);
        assertEq(pool.reserved(), 0);
        assertEq(pool.maxWithdraw(lender), 1_000 ether);
    }

    function testUnsecuredDefaultReducesShareValueAndRecoveryRestoresAssets() public {
        _line();
        _spend(10 ether, keccak256("unpaid"));
        pool.closeLoan(LOAN);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.recognizeLoss(LOAN);
        vm.warp(block.timestamp + 8 days);
        pool.recognizeLoss(LOAN);
        assertEq(pool.totalAssets(), 990 ether);
        assertEq(pool.getLoan(LOAN).debt, 10 ether);
        assertEq(pool.getLoan(LOAN).loss, 10 ether);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 10 ether);
        assertEq(pool.totalAssets(), 1_000 ether);
        assertEq(pool.getLoan(LOAN).loss, 0);
    }

    function testDirectDonationsCannotManipulateSharePrice() public {
        usdt.mint(address(pool), 1_000 ether);
        assertEq(pool.totalAssets(), 1_000 ether);
        assertEq(pool.previewDeposit(1 ether), 1 ether);
    }

    function testNoUnauthorizedLenderWithdrawal() public {
        vm.prank(borrower);
        vm.expectRevert();
        pool.withdraw(1 ether, borrower, lender);
        assertEq(pool.balanceOf(lender), 1_000 ether);
    }

    function testFuzzPrincipalConservationAfterSpendAndRepay(uint128 spendSeed, uint128 repaySeed) public {
        _line();
        uint256 spend = bound(spendSeed, 1, 10 ether);
        uint256 repayAmount = bound(repaySeed, 1, spend);
        _spend(spend, keccak256("fuzz"));
        vm.prank(borrower);
        pool.repayLoan(LOAN, repayAmount);
        pool.closeLoan(LOAN);
        assertEq(pool.totalAssets(), 1_000 ether);
        assertEq(pool.liquidity() + pool.outstanding(), pool.totalAssets());
        assertEq(pool.outstanding(), spend - repayAmount);
        assertEq(usdt.balanceOf(address(pool)), pool.liquidity());
        uint256 maximum = pool.maxWithdraw(lender);
        vm.prank(lender);
        pool.withdraw(maximum, lender, lender);
        assertEq(pool.liquidity(), 0);
        assertEq(pool.totalAssets(), spend - repayAmount);
    }

    function testFeeOnTransferUSDTDepositRejectedAtomically() public {
        FinanceTaxToken taxed = new FinanceTaxToken(18);
        TabProtocol local =
            TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(taxed), address(this))));
        TabLendingPool localPool = new TabLendingPool(address(local));
        taxed.mint(lender, 10 ether);
        taxed.setTax(true);
        _approve(address(taxed), lender, address(localPool));
        vm.prank(lender);
        vm.expectRevert(TabUSDTLiquidity.TransferFailed.selector);
        localPool.deposit(10 ether, lender);
        assertEq(localPool.totalSupply(), 0);
        assertEq(taxed.balanceOf(lender), 10 ether);
    }

    function testTokenCallbackCannotReenterDeposit() public {
        FinanceCallbackToken callback = new FinanceCallbackToken();
        TabProtocol local = TabProtocol(
            deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(callback), address(this)))
        );
        TabLendingPool localPool = new TabLendingPool(address(local));
        callback.mint(lender, 10 ether);
        _approve(address(callback), lender, address(localPool));
        callback.arm(address(localPool));
        vm.prank(lender);
        localPool.deposit(10 ether, lender);
        assertTrue(callback.blocked());
        assertEq(callback.blockedError(), bytes4(keccak256("ReentrancyGuardReentrantCall()")));
        assertEq(localPool.totalAssets(), 10 ether);
    }

    function testOnlyShareOwnerOrApprovedSpenderCanRedeem() public {
        vm.prank(borrower);
        vm.expectRevert();
        pool.redeem(1 ether, borrower, lender);
        vm.prank(lender);
        pool.approve(borrower, 1 ether);
        vm.prank(borrower);
        pool.redeem(1 ether, lender, lender);
        assertEq(pool.balanceOf(lender), 999 ether);
    }
}
