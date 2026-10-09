// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {
    FinanceBase,
    FinanceToken,
    FinanceTaxToken,
    FinanceFeed,
    FinanceMarket
} from "./TabFinanceTestBase.sol";
import {TabStockLending} from "../src/TabStockLending.sol";
import {TabUSDTLiquidity} from "../src/TabUSDTLiquidity.sol";
import {TabHolderAccess} from "../src/TabHolderAccess.sol";

contract TabStockLendingTest is FinanceBase {
    TabStockLending pool;
    FinanceToken stock;
    FinanceFeed stockFeed;
    FinanceFeed usdtFeed;
    FinanceMarket market;

    function setUp() public override {
        super.setUp();
        _holders(protocol);
        stock = new FinanceToken(6);
        stockFeed = new FinanceFeed(8, 100e8);
        usdtFeed = new FinanceFeed(8, 1e8);
        market = new FinanceMarket();
        market.set(address(stock), true);
        pool = new TabStockLending(address(protocol), address(usdtFeed), 1 hours);
        pool.configureCollateral(
            address(stock), address(stockFeed), address(market), 1 hours, 5000, 7000, 500, 10_000 ether
        );
        _approve(address(usdt), lender, address(pool));
        _approve(address(usdt), borrower, address(pool));
        _approve(address(usdt), merchant, address(pool));
        _approve(address(stock), borrower, address(pool));
        stock.mint(borrower, 100e6);
        vm.prank(lender);
        pool.deposit(5_000 ether, lender);
    }

    function _loan() internal {
        vm.prank(borrower);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
    }

    function testHolderLossBlocksBorrowingButKeepsRepaymentAndCollateralRelease() public {
        _loan();
        vm.prank(borrower);
        holderToken.transfer(merchant, 1 ether);
        vm.prank(borrower);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        pool.borrow(keccak256("another-loan"), address(stock), 2e6, 1 ether);
        vm.prank(borrower);
        pool.addCollateral(LOAN, 1e6);
        vm.prank(borrower);
        pool.repay(LOAN, 1_000 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 21e6);
        assertEq(pool.getLoan(LOAN).collateral, 0);
        assertEq(pool.getLoan(LOAN).debt, 0);
    }

    function testStockSixDecimalsOracleEightDecimalsPricedInUSDT() public {
        uint256 before_ = usdt.balanceOf(borrower);
        _loan();
        assertEq(usdt.balanceOf(borrower) - before_, 1_000 ether);
        assertEq(stock.balanceOf(address(pool)), 20e6);
        assertEq(pool.collateralLiability(address(stock)), 20e6);
        (uint256 value, uint256 maximum, uint256 threshold, bool liquidatable) = pool.loanHealth(LOAN);
        assertEq(value, 2_000 ether);
        assertEq(maximum, 1_000 ether);
        assertEq(threshold, 1_400 ether);
        assertFalse(liquidatable);
        assertEq(pool.totalAssets(), 5_000 ether);
        assertEq(pool.maxWithdraw(lender), 4_000 ether);
    }

    function testQuoteBeforeBorrowMatchesLoanAndRejectsClosedMarket() public {
        (uint256 value, uint256 maximum, uint256 threshold) = pool.collateralQuote(address(stock), 20e6);
        assertEq(value, 2_000 ether);
        assertEq(maximum, 1_000 ether);
        assertEq(threshold, 1_400 ether);
        assertTrue(pool.marketOpen(address(stock)));
        assertFalse(pool.marketOpen(address(0x1234)));
        market.set(address(stock), false);
        assertFalse(pool.marketOpen(address(stock)));
        vm.expectRevert(TabStockLending.MarketClosed.selector);
        pool.collateralQuote(address(stock), 20e6);
    }

    function testUSDTUsdFeedHandlesDepegAndAppreciation() public {
        usdtFeed.set(2e8);
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
        vm.prank(borrower);
        pool.borrow(LOAN, address(stock), 20e6, 500 ether);
        (uint256 value,,,) = pool.loanHealth(LOAN);
        assertEq(value, 1_000 ether);
        usdtFeed.set(5e7);
        (value,,,) = pool.loanHealth(LOAN);
        assertEq(value, 4_000 ether);
    }

    function testBorrowAboveLtvAndLiquidityReverts() public {
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether + 1);
        vm.prank(lender);
        pool.withdraw(4_500 ether, lender, lender);
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
    }

    function testDefaultMarketClosedAndWhitelistMissingPreventBorrow() public {
        FinanceToken unknown = new FinanceToken(6);
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.borrow(LOAN, address(unknown), 1e6, 1 ether);
        market.set(address(stock), false);
        vm.prank(borrower);
        vm.expectRevert(TabStockLending.MarketClosed.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
        vm.expectRevert(TabStockLending.MarketClosed.selector);
        pool.configureCollateral(
            address(unknown), address(stockFeed), address(market), 1 hours, 5000, 7000, 500, 100 ether
        );
    }

    function testStaleCollateralAndStablecoinPricesStopBorrow() public {
        vm.warp(block.timestamp + 1 hours + 1);
        usdtFeed.set(1e8);
        vm.prank(borrower);
        vm.expectRevert(TabStockLending.PriceUnavailable.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
        stockFeed.set(100e8);
        usdtFeed.setRound(1, 1, block.timestamp - 1 hours - 1);
        vm.prank(borrower);
        vm.expectRevert(TabStockLending.PriceUnavailable.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
    }

    function testNegativeZeroFutureAndIncompleteOracleRoundsRejected() public {
        stockFeed.set(-1);
        vm.prank(borrower);
        vm.expectRevert(TabStockLending.PriceUnavailable.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
        stockFeed.set(0);
        vm.prank(borrower);
        vm.expectRevert(TabStockLending.PriceUnavailable.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
        stockFeed.set(100e8);
        stockFeed.setRound(9, 8, block.timestamp);
        vm.prank(borrower);
        vm.expectRevert(TabStockLending.PriceUnavailable.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
        stockFeed.setRound(9, 9, block.timestamp + 1);
        vm.prank(borrower);
        vm.expectRevert(TabStockLending.PriceUnavailable.selector);
        pool.borrow(LOAN, address(stock), 20e6, 1_000 ether);
    }

    function testRepayAndWithdrawAllCollateralWhenPausedClosedAndStale() public {
        _loan();
        pool.setPaused(true);
        market.set(address(stock), false);
        vm.warp(block.timestamp + 2 days);
        vm.prank(borrower);
        pool.repay(LOAN, 1_000 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 20e6);
        assertEq(pool.outstanding(), 0);
        assertEq(pool.collateralLiability(address(stock)), 0);
        assertEq(stock.balanceOf(borrower), 100e6);
        uint256 shares = pool.balanceOf(lender);
        vm.prank(lender);
        assertEq(pool.redeem(shares, lender, lender), 5_000 ether);
    }

    function testOnlyBorrowerWithdrawsAndActiveDebtStaysCollateralized() public {
        _loan();
        vm.prank(merchant);
        vm.expectRevert(TabUSDTLiquidity.Authority.selector);
        pool.withdrawCollateral(LOAN, 1e6);
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.withdrawCollateral(LOAN, 1e6);
        vm.prank(borrower);
        pool.repay(LOAN, 100 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 2e6);
        assertEq(pool.getLoan(LOAN).collateral, 18e6);
    }

    function testCollateralTopUpAllowedWhenPausedAndClosed() public {
        _loan();
        pool.setPaused(true);
        market.set(address(stock), false);
        vm.prank(borrower);
        pool.addCollateral(LOAN, 10e6);
        assertEq(pool.getLoan(LOAN).collateral, 30e6);
    }

    function testHealthyLoanCannotBeLiquidated() public {
        _loan();
        vm.prank(merchant);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.liquidate(LOAN, 400 ether, 0);
    }

    function testLiquidatorPaysUSDTForDocumentedCollateralBonus() public {
        _loan();
        stockFeed.set(60e8);
        (, uint256 collateral) = pool.liquidationQuote(LOAN, 400 ether);
        assertEq(collateral, 7e6);
        uint256 before_ = usdt.balanceOf(merchant);
        vm.prank(merchant);
        pool.liquidate(LOAN, 400 ether, collateral);
        assertEq(before_ - usdt.balanceOf(merchant), 400 ether);
        assertEq(stock.balanceOf(merchant), 7e6);
        assertEq(pool.getLoan(LOAN).debt, 600 ether);
        assertEq(pool.getLoan(LOAN).collateral, 13e6);
        assertEq(pool.outstanding(), 600 ether);
        assertEq(pool.liquidity(), 4_400 ether);
        assertEq(pool.totalAssets(), 5_000 ether);
    }

    function testLiquidatorMinimumCollateralAndMarketGate() public {
        _loan();
        stockFeed.set(60e8);
        vm.prank(merchant);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.liquidate(LOAN, 400 ether, 7e6 + 1);
        market.set(address(stock), false);
        vm.prank(merchant);
        vm.expectRevert(TabStockLending.MarketClosed.selector);
        pool.liquidate(LOAN, 400 ether, 0);
    }

    function testInsolventLiquidationRecognizesLossAndBorrowerStillOwes() public {
        _loan();
        stockFeed.set(5e8);
        uint256 covered = uint256(100 ether) * 10_000 / 10_500;
        (uint256 maximum, uint256 seized) = pool.liquidationQuote(LOAN, covered);
        assertEq(maximum, covered);
        assertEq(seized, 20e6);
        vm.prank(merchant);
        pool.liquidate(LOAN, covered, seized);
        pool.recognizeBadDebt(LOAN);
        assertEq(pool.outstanding(), 0);
        assertEq(pool.totalAssets(), 4_000 ether + covered);
        assertEq(pool.getLoan(LOAN).debt, 1_000 ether - covered);
        assertEq(pool.getLoan(LOAN).loss, 1_000 ether - covered);
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.recognizeBadDebt(LOAN);
        vm.prank(borrower);
        pool.repay(LOAN, 1_000 ether - covered);
        assertEq(pool.totalAssets(), 5_000 ether);
    }

    function testRiskTermsSnapshotCannotBeChangedForExistingBorrower() public {
        _loan();
        pool.configureCollateral(
            address(stock), address(stockFeed), address(market), 1 hours, 1000, 2000, 100, 100 ether
        );
        assertEq(pool.getLoan(LOAN).terms.borrowLtvBps, 5000);
        assertEq(pool.getLoan(LOAN).terms.liquidationLtvBps, 7000);
        assertEq(pool.getAsset(address(stock)).borrowLtvBps, 1000);
        pool.disableCollateral(address(stock));
        vm.prank(borrower);
        pool.repay(LOAN, 1_000 ether);
        vm.prank(borrower);
        pool.withdrawCollateral(LOAN, 20e6);
    }

    function testWhitelistAuthorityAndUnsafeRiskTerms() public {
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.Authority.selector);
        pool.disableCollateral(address(stock));
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.configureCollateral(
            address(stock), address(stockFeed), address(market), 1 hours, 7000, 7000, 500, 100 ether
        );
        vm.expectRevert(TabUSDTLiquidity.Terms.selector);
        pool.configureCollateral(
            address(stock), address(stockFeed), address(market), 1 hours, 5000, 9000, 1500, 100 ether
        );
    }

    function testFeeOnTransferCollateralRejectedAndNoDebtMinted() public {
        FinanceTaxToken taxed = new FinanceTaxToken(6);
        market.set(address(taxed), true);
        pool.configureCollateral(
            address(taxed), address(stockFeed), address(market), 1 hours, 5000, 7000, 500, 100 ether
        );
        taxed.mint(borrower, 10e6);
        taxed.setTax(true);
        _approve(address(taxed), borrower, address(pool));
        vm.prank(borrower);
        vm.expectRevert(TabUSDTLiquidity.TransferFailed.selector);
        pool.borrow(LOAN, address(taxed), 10e6, 100 ether);
        assertEq(pool.outstanding(), 0);
        assertEq(pool.getLoan(LOAN).borrower, address(0));
        assertEq(taxed.balanceOf(borrower), 10e6);
    }

    function testFuzzConservationAcrossBorrowRepayWithdraw(uint128 debtSeed, uint128 repaySeed) public {
        uint256 principal = bound(debtSeed, 1, 1_000 ether);
        uint256 repayAmount = bound(repaySeed, 1, principal);
        vm.prank(borrower);
        pool.borrow(LOAN, address(stock), 20e6, principal);
        vm.prank(borrower);
        pool.repay(LOAN, repayAmount);
        assertEq(pool.totalAssets(), 5_000 ether);
        assertEq(pool.outstanding(), principal - repayAmount);
        assertEq(usdt.balanceOf(address(pool)), pool.liquidity());
        assertEq(stock.balanceOf(address(pool)), pool.collateralLiability(address(stock)));
        uint256 maximum = pool.maxWithdraw(lender);
        vm.prank(lender);
        pool.withdraw(maximum, lender, lender);
        assertEq(pool.liquidity(), 0);
        assertEq(pool.totalAssets(), principal - repayAmount);
    }
}
