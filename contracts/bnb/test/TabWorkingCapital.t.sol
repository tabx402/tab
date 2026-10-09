// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {FinanceBase, FinanceToken, FinanceFeed, FinanceTaxToken, FinanceCallbackToken} from "./TabFinanceTestBase.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabLendingPool} from "../src/TabLendingPool.sol";
import {TabUSDTLiquidity} from "../src/TabUSDTLiquidity.sol";
import {TabHolderAccess} from "../src/TabHolderAccess.sol";
import {TabPriceOracle} from "../src/TabPriceOracle.sol";

contract RejectNativeCollateral { receive() external payable { revert("native rejected"); } }

contract ReenterNativeCollateral {
    TabLendingPool immutable target;
    bytes32 immutable loan;
    bool public blocked;
    bytes4 public blockedError;
    constructor(TabLendingPool pool_, bytes32 id) { target = pool_; loan = id; }
    receive() external payable {
        (bool ok, bytes memory reason) = address(target).call(abi.encodeCall(target.withdrawCollateral, (loan, 1)));
        blocked = !ok;
        if (reason.length >= 4) blockedError = bytes4(reason);
    }
}

contract TabWorkingCapitalTest is FinanceBase {
    TabLendingPool pool;
    FinanceFeed bnbFeed;
    FinanceFeed stableFeed;
    TabPriceOracle oracle;
    address constant WBNB = 0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c;

    function setUp() public override {
        super.setUp();
        _holders(protocol);
        FinanceToken wrapped = new FinanceToken(18);
        vm.etch(WBNB, address(wrapped).code);
        bnbFeed = new FinanceFeed(8, 1000e8);
        stableFeed = new FinanceFeed(8, 1e8);
        oracle = new TabPriceOracle(WBNB, address(usdt), address(bnbFeed), address(stableFeed), 1 hours, 1 hours);
        pool = new TabLendingPool(address(protocol), address(oracle));
        vm.deal(borrower, 10 ether);
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
        vm.prank(borrower);
        pool.pledgeCollateral{value: 0.1 ether}(LOAN);
    }

    function _oracle(address stable) internal returns (address) {
        return address(new TabPriceOracle(WBNB, stable, address(new FinanceFeed(8, 1000e8)), address(new FinanceFeed(8, 1e8)), 1 hours, 1 hours));
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

    function testHolderLossBlocksNewDepositsButKeepsRedemption() public {
        vm.prank(lender);
        holderToken.transfer(merchant, 1 ether);
        assertFalse(pool.hasTabAccess(lender));
        assertEq(pool.maxDeposit(lender), 0);
        assertEq(pool.maxMint(lender), 0);
        vm.prank(lender);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        pool.deposit(1 ether, lender);
        vm.prank(lender);
        assertEq(pool.redeem(1_000 ether, lender, lender), 1_000 ether);
    }

    function testDepositChecksCallerAndShareReceiver() public {
        vm.prank(lender);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        pool.deposit(1 ether, merchant);
        _approve(address(usdt), merchant, address(pool));
        vm.prank(merchant);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        pool.deposit(1 ether, lender);
    }

    function testMissingOfficialTokenFailsClosed() public {
        TabProtocol empty = TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(usdt), address(this))));
        TabLendingPool emptyPool = new TabLendingPool(address(empty), address(oracle));
        assertFalse(emptyPool.hasTabAccess(lender));
        assertEq(emptyPool.maxDeposit(lender), 0);
        vm.prank(lender);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        emptyPool.deposit(1 ether, lender);
    }

    function testHolderLossBlocksAcceptAndDelegatedSpendingButKeepsRecovery() public {
        pool.approveLoan(_terms());
        vm.prank(borrower);
        holderToken.transfer(merchant, 1 ether);
        vm.prank(borrower);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        pool.acceptLoan(LOAN);
        vm.prank(merchant);
        holderToken.transfer(borrower, 1 ether);
        vm.prank(borrower);
        pool.acceptLoan(LOAN);
        vm.prank(borrower);
        pool.pledgeCollateral{value: 0.1 ether}(LOAN);
        _spend(1 ether, keccak256("holder-paid"));
        vm.prank(borrower);
        holderToken.transfer(merchant, 1 ether);
        vm.prank(signer);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("after-sale"), RECEIPT);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 1 ether);
        vm.prank(borrower);
        pool.closeLoan(LOAN);
        assertEq(pool.outstanding(), 0);
        assertEq(pool.reserved(), 0);
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

    function testSecuredResidualDefaultRequiresExhaustedCollateralAndRecoveryRestoresAssets() public {
        _line();
        _spend(10 ether, keccak256("unpaid"));
        pool.closeLoan(LOAN);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.recognizeLoss(LOAN);
        vm.warp(block.timestamp + 8 days);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.recognizeLoss(LOAN);
        bnbFeed.set(50e8);
        stableFeed.set(1e8);
        _approve(address(usdt), merchant, address(pool));
        (uint256 repaid, uint256 seized) = pool.liquidationQuote(LOAN, 10 ether);
        vm.prank(merchant);
        pool.liquidateLoan(LOAN, 10 ether, seized);
        pool.recognizeLoss(LOAN);
        assertEq(pool.totalAssets(), 990 ether + repaid);
        assertEq(pool.getLoan(LOAN).debt, 10 ether - repaid);
        assertEq(pool.getLoan(LOAN).loss, 10 ether - repaid);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 10 ether - repaid);
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
        TabLendingPool localPool = new TabLendingPool(address(local), _oracle(address(taxed)));
        _holders(local);
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
        _holders(local);
        TabLendingPool localPool = new TabLendingPool(address(local), _oracle(address(callback)));
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

    function testImmutableOracleAndRiskConfiguration() public view {
        assertEq(pool.securedCreditVersion(), 1);
        assertEq(address(pool.collateralOracle()), address(oracle));
        assertEq(pool.WBNB(), WBNB);
        assertEq(pool.LTV_BPS(), 5000);
        assertEq(pool.LIQUIDATION_BPS(), 7500);
        assertEq(pool.LIQUIDATION_BONUS_BPS(), 500);
        (uint256 value, uint256 borrowing, uint256 liquidationDebt) = pool.collateralQuote(0.1 ether);
        assertEq(value, 100 ether);
        assertEq(borrowing, 50 ether);
        assertEq(liquidationDebt, 75 ether);
    }

    function testConstructorRejectsWrongCollateralAndQuoteBindings() public {
        FinanceToken wrong = new FinanceToken(18);
        TabPriceOracle wrongCollateral = new TabPriceOracle(address(wrong), address(usdt), address(bnbFeed), address(stableFeed), 1 hours, 1 hours);
        vm.expectRevert(TabLendingPool.Collateral.selector);
        new TabLendingPool(address(protocol), address(wrongCollateral));
        address wrongQuote = _oracle(address(wrong));
        vm.expectRevert(TabLendingPool.Collateral.selector);
        new TabLendingPool(address(protocol), wrongQuote);
    }

    function testApprovalAndAcceptanceDoNotCreateUnsecuredSpendingPower() public {
        pool.approveLoan(_terms());
        vm.prank(borrower);
        pool.acceptLoan(LOAN);
        vm.prank(signer);
        vm.expectRevert(TabLendingPool.Collateral.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("no-collateral"), RECEIPT);
        assertEq(pool.outstanding(), 0);
        vm.prank(borrower);
        pool.pledgeCollateral{value: 0.002 ether}(LOAN);
        _spend(1 ether, keccak256("exact-ltv"));
        vm.prank(signer);
        vm.expectRevert(TabLendingPool.Collateral.selector);
        pool.spendLoan(LOAN, merchant, 1, 1, keccak256("above-ltv"), RECEIPT);
    }

    function testCollateralIsIsolatedAndOnlyBorrowerCanRecoverIt() public {
        _line();
        vm.deal(lender, 1 ether);
        vm.prank(lender);
        vm.expectRevert(TabUSDTLiquidity.Authority.selector);
        pool.pledgeCollateral{value: 1}(LOAN);
        vm.prank(lender);
        vm.expectRevert(TabUSDTLiquidity.Authority.selector);
        pool.withdrawCollateral(LOAN, 1);
        bytes32 other = keccak256("other-loan");
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Authority.selector);
        pool.pledgeCollateral{value: 1}(other);
        assertEq(pool.collateral(LOAN), 0.1 ether);
        assertEq(pool.totalCollateral(), 0.1 ether);
        assertEq(address(pool).balance, 0.1 ether);
    }

    function testWithdrawalKeepsDebtWithinLtvAndRepaymentUnlocksExit() public {
        _line();
        _spend(10 ether, keccak256("debt"));
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 0.08 ether);
        vm.prank(borrower);
        vm.expectRevert(TabLendingPool.Collateral.selector);
        pool.withdrawCollateral(LOAN, 1);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 10 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 0.02 ether);
        assertEq(pool.totalCollateral(), 0);
    }

    function testHolderLossPauseAndOracleFailureKeepTopupRepaymentAndDebtFreeExit() public {
        _line();
        _spend(10 ether, keccak256("debt"));
        pool.setPaused(true);
        vm.prank(borrower);
        holderToken.transfer(merchant, 1 ether);
        bnbFeed.set(0);
        vm.prank(borrower);
        pool.pledgeCollateral{value: 0.01 ether}(LOAN);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 10 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 0.11 ether);
        vm.prank(borrower);
        pool.closeLoan(LOAN);
        assertEq(pool.outstanding(), 0);
        assertEq(pool.totalCollateral(), 0);
    }

    function testStaleOracleStopsNewDebtAndLiquidation() public {
        _line();
        _spend(10 ether, keccak256("debt"));
        bnbFeed.setRound(1, 1, block.timestamp - 2 hours);
        vm.prank(signer);
        vm.expectRevert(TabPriceOracle.InvalidFeed.selector);
        pool.spendLoan(LOAN, merchant, 1 ether, 1, keccak256("stale"), RECEIPT);
        vm.expectRevert(TabPriceOracle.InvalidFeed.selector);
        pool.liquidationQuote(LOAN, 10 ether);
        vm.prank(borrower);
        pool.repayLoan(LOAN, 10 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 0.1 ether);
    }

    function testUSDTAppreciationLowersNativeBorrowingPower() public {
        _line();
        stableFeed.set(2e8);
        (,uint256 borrowing,) = pool.collateralQuote(0.1 ether);
        assertEq(borrowing, 25 ether);
        _spend(10 ether, keccak256("first"));
        _spend(10 ether, keccak256("second"));
        vm.prank(signer);
        vm.expectRevert(TabLendingPool.Collateral.selector);
        pool.spendLoan(LOAN, merchant, 6 ether, 1, keccak256("above"), RECEIPT);
    }

    function testHealthyCollateralCannotBeLiquidatedUntilExpiryGrace() public {
        _line();
        _spend(10 ether, keccak256("debt"));
        vm.expectRevert(TabLendingPool.Collateral.selector);
        pool.liquidationQuote(LOAN, 10 ether);
        vm.warp(pool.getLoan(LOAN).expiresAt + 1 days);
        bnbFeed.set(1000e8); stableFeed.set(1e8);
        vm.expectRevert(TabLendingPool.Collateral.selector);
        pool.liquidationQuote(LOAN, 10 ether);
        vm.warp(block.timestamp + 1);
        (uint256 repaid, uint256 seized) = pool.liquidationQuote(LOAN, 10 ether);
        assertEq(repaid, 10 ether);
        assertEq(seized, 0.0105 ether);
    }

    function testLiquidationRecoversUsdtClosesLineAndPreservesBorrowerExcess() public {
        _line();
        _spend(10 ether, keccak256("debt"));
        bnbFeed.set(120e8);
        _approve(address(usdt), merchant, address(pool));
        (uint256 repaid, uint256 seized) = pool.liquidationQuote(LOAN, 10 ether);
        assertEq(repaid, 10 ether);
        assertEq(seized, 0.0875 ether);
        uint256 beforeNative = merchant.balance;
        uint256 beforeUsdt = usdt.balanceOf(merchant);
        vm.prank(merchant);
        pool.liquidateLoan(LOAN, 10 ether, seized);
        assertEq(merchant.balance - beforeNative, seized);
        assertEq(beforeUsdt - usdt.balanceOf(merchant), repaid);
        assertEq(pool.reserved(), 0);
        assertEq(pool.outstanding(), 0);
        assertTrue(pool.getLoan(LOAN).closed);
        assertEq(pool.collateral(LOAN), 0.0125 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 0.0125 ether);
        assertEq(pool.totalCollateral(), 0);
        assertEq(pool.totalAssets(), 1_000 ether);
    }

    function testPartialLiquidationSlippageAndApprovalAreAtomic() public {
        _line();
        _spend(10 ether, keccak256("debt"));
        bnbFeed.set(120e8);
        (uint256 repaid, uint256 seized) = pool.liquidationQuote(LOAN, 4 ether);
        assertEq(repaid, 4 ether);
        assertEq(seized, 0.035 ether);
        _approve(address(usdt), merchant, address(pool));
        vm.prank(merchant);
        vm.expectRevert(TabLendingPool.Collateral.selector);
        pool.liquidateLoan(LOAN, 4 ether, seized + 1);
        assertEq(pool.collateral(LOAN), 0.1 ether);
        assertEq(pool.reserved(), 30 ether);
        vm.prank(merchant);
        pool.liquidateLoan(LOAN, 4 ether, seized);
        assertEq(pool.getLoan(LOAN).debt, 6 ether);
        assertEq(pool.collateral(LOAN), 0.065 ether);
        assertEq(pool.outstanding(), 6 ether);
        assertEq(pool.totalAssets(), 1_000 ether);
    }

    function testNativeReceiverFailureRollsBackCollateralWithdrawal() public {
        _line();
        RejectNativeCollateral rejector = new RejectNativeCollateral();
        vm.etch(borrower, address(rejector).code);
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.TransferFailed.selector);
        pool.withdrawCollateral(LOAN, 0.1 ether);
        assertEq(pool.collateral(LOAN), 0.1 ether);
        assertEq(pool.totalCollateral(), 0.1 ether);
    }

    function testNativeReceiverCannotReenterCollateralWithdrawal() public {
        _line();
        ReenterNativeCollateral attacker = new ReenterNativeCollateral(pool, LOAN);
        vm.etch(borrower, address(attacker).code);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 0.05 ether);
        assertTrue(ReenterNativeCollateral(payable(borrower)).blocked());
        assertEq(ReenterNativeCollateral(payable(borrower)).blockedError(), bytes4(keccak256("ReentrancyGuardReentrantCall()")));
        assertEq(pool.collateral(LOAN), 0.05 ether);
        assertEq(pool.totalCollateral(), 0.05 ether);
    }

    function testWorthlessNativeDustCannotPreventResidualLossRecovery() public {
        _line();
        _spend(500, keccak256("tiny-debt"));
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 0.1 ether - 1);
        bnbFeed.set(1);
        _approve(address(usdt), merchant, address(pool));
        (uint256 repaid, uint256 seized) = pool.liquidationQuote(LOAN, 500);
        assertEq(repaid, 1);
        assertEq(seized, 1);
        vm.prank(merchant);
        pool.liquidateLoan(LOAN, 500, 1);
        assertEq(pool.collateral(LOAN), 0);
        assertEq(pool.getLoan(LOAN).debt, 499);
        vm.warp(block.timestamp + 8 days);
        pool.recognizeLoss(LOAN);
        assertEq(pool.getLoan(LOAN).loss, 499);
    }

    function testFuzzLiquidationConservesCollateralAndPrincipal(uint128 debtSeed, uint128 priceSeed) public {
        _line();
        uint256 debt = bound(debtSeed, 1 ether, 10 ether);
        _spend(debt, keccak256("liquidation-fuzz"));
        uint256 price = bound(priceSeed, 1e8, 10e8);
        bnbFeed.set(int256(price));
        _approve(address(usdt), merchant, address(pool));
        (uint256 repaid, uint256 seized) = pool.liquidationQuote(LOAN, debt);
        assertLe(repaid, debt);
        assertLe(seized, 0.1 ether);
        vm.prank(merchant);
        pool.liquidateLoan(LOAN, debt, seized);
        assertEq(pool.outstanding(), debt - repaid);
        assertEq(pool.totalAssets(), 1_000 ether);
        assertEq(pool.totalCollateral(), 0.1 ether - seized);
        assertEq(address(pool).balance, pool.totalCollateral());
        assertEq(usdt.balanceOf(address(pool)), pool.liquidity());
        assertEq(pool.getLoan(LOAN).debt + pool.getLoan(LOAN).repaid, debt);
    }
}
