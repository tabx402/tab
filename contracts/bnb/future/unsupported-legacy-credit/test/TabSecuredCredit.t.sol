// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {TabBase, MockToken} from "./TabBNB.t.sol";
import {TabBacking} from "../src/TabBacking.sol";
import {TabPriceOracle} from "../src/TabPriceOracle.sol";
import {TabTypes as T} from "../src/TabTypes.sol";

contract MockFeed {
    uint8 public decimals = 8;
    int256 public answer;
    uint256 public updated;
    uint80 public round = 1;
    uint80 public answered = 1;
    constructor(int256 price) { set(price); }
    function set(int256 price) public { answer = price; updated = block.timestamp; }
    function stale(uint256 at) external { updated = at; }
    function incomplete() external { answered = 0; }
    function latestRoundData() external view returns (uint80, int256, uint256, uint256, uint80) {
        return (round, answer, updated, updated, answered);
    }
}

contract TabSecuredCreditTest is TabBase {
    MockToken stock;
    MockFeed feed;
    MockFeed usd;
    TabPriceOracle oracle;
    function setUp() public override {
        super.setUp();
        stock = new MockToken(8);
        feed = new MockFeed(100e8);
        usd = new MockFeed(1e8);
        oracle = new TabPriceOracle(address(stock), address(u), address(feed), address(usd), 1 hours, 1 hours);
        b.configureCollateral(address(stock), address(oracle), 5000, 7000, 500, 1000 ether);
        stock.mint(alice, 10e8);
        vm.prank(alice);
        stock.approve(address(b), type(uint256).max);
        vm.prank(lender);
        b.openCreditWithCollateral(T.CreditTerms(CREDIT, A, signer, 100 ether, 100 ether, 100 ether, uint64(block.timestamp + 2 days), 7, _providers()), address(stock));
        vm.prank(alice);
        b.acceptCredit(CREDIT);
    }
    function _pledgeAndSpend() internal {
        vm.startPrank(alice);
        b.pledgeCollateral(CREDIT, 2e8);
        b.spendCredit(CREDIT, merchant, 100 ether, 1, POLICY, EVIDENCE);
        vm.stopPrank();
    }
    function testCustodyDoesNotCreateBorrowingPower() public {
        vm.startPrank(alice);
        b.backAgent(A, address(stock), 2e8);
        vm.expectRevert(TabBacking.Collateral.selector);
        b.spendCredit(CREDIT, merchant, 1 ether, 1, POLICY, EVIDENCE);
        b.pledgeCollateral(CREDIT, 2e8);
        assertEq(b.borrowingPower(CREDIT), 100 ether);
        b.spendCredit(CREDIT, merchant, 100 ether, 1, POLICY, EVIDENCE);
        vm.expectRevert(TabBacking.Collateral.selector);
        b.withdrawCollateral(CREDIT, 1);
        vm.stopPrank();
    }
    function testPriceDropLiquidationPaysLenderAndPreservesExcess() public {
        _pledgeAndSpend();
        feed.set(60e8);
        vm.prank(carol);
        b.liquidateCredit(CREDIT, 100 ether, 175000000);
        T.Credit memory c = b.getCredit(CREDIT);
        assertEq(c.outstanding, 0);
        assertEq(c.available, 100 ether);
        assertTrue(c.closed);
        assertEq(stock.balanceOf(carol), 175000000);
        vm.prank(lender);
        b.withdrawCredit(CREDIT, 100 ether);
        vm.prank(alice);
        b.withdrawCollateral(CREDIT, 25000000);
        assertEq(b.tokenLiability(address(stock)), 0);
        assertEq(b.tokenLiability(address(u)), 0);
    }
    function testHealthyPositionCannotBeLiquidated() public {
        _pledgeAndSpend();
        vm.prank(carol);
        vm.expectRevert(TabBacking.Collateral.selector);
        b.liquidateCredit(CREDIT, 10 ether, 0);
    }
    function testStaleOracleBlocksNewDebtAndLiquidationButRepaidExitWorks() public {
        _pledgeAndSpend();
        feed.stale(block.timestamp - 2 hours);
        vm.prank(carol);
        vm.expectRevert(TabPriceOracle.InvalidFeed.selector);
        b.liquidateCredit(CREDIT, 10 ether, 0);
        vm.startPrank(alice);
        b.repayCredit(CREDIT, 100 ether);
        b.withdrawCollateral(CREDIT, 2e8);
        vm.stopPrank();
        assertEq(stock.balanceOf(address(b)), 0);
    }
    function testOracleRejectsZeroFutureAndIncompleteRounds() public {
        feed.set(0);
        vm.expectRevert(TabPriceOracle.InvalidFeed.selector); oracle.price();
        feed.set(100e8); feed.stale(block.timestamp + 1);
        vm.expectRevert(TabPriceOracle.InvalidFeed.selector); oracle.price();
        feed.set(100e8); feed.incomplete();
        vm.expectRevert(TabPriceOracle.InvalidFeed.selector); oracle.price();
    }
    function testQuoteDepegReducesBorrowingPower() public {
        vm.prank(alice); b.pledgeCollateral(CREDIT, 2e8);
        usd.set(2e8);
        assertEq(b.borrowingPower(CREDIT), 50 ether);
    }
    function testExpiryGraceAndSlippage() public {
        _pledgeAndSpend();
        vm.warp(block.timestamp + 3 days + 1);
        feed.set(100e8); usd.set(1e8);
        vm.prank(carol);
        vm.expectRevert(TabBacking.Collateral.selector);
        b.liquidateCredit(CREDIT, 100 ether, 106000000);
        vm.prank(carol); b.liquidateCredit(CREDIT, 100 ether, 105000000);
        assertEq(stock.balanceOf(carol), 105000000);
    }
    function testPauseCannotBlockRepaymentOrRepaidCollateralWithdrawal() public {
        _pledgeAndSpend();
        b.pauseCollateral(address(stock), true);
        vm.startPrank(alice);
        b.repayCredit(CREDIT, 100 ether);
        b.withdrawCollateral(CREDIT, 2e8);
        vm.stopPrank();
    }
    function testCollateralCannotSecureTwoLoans() public {
        _pledgeAndSpend();
        bytes32 second = keccak256("second");
        vm.prank(lender);
        b.openCreditWithCollateral(T.CreditTerms(second, A, signer, 100 ether, 100 ether, 100 ether, uint64(block.timestamp + 2 days), 7, _providers()), address(stock));
        vm.startPrank(alice);
        b.acceptCredit(second);
        vm.expectRevert(TabBacking.Collateral.selector);
        b.spendCredit(second, merchant, 1 ether, 1, POLICY, EVIDENCE);
        vm.stopPrank();
    }
}

contract TabHolderFeesTest is TabBase {
    function testFeeIsHalfPercentBeforeOfficialTokenConfigured() public view { assertEq(p.effectiveFeeBps(alice), 50); }
    function testHolderExemptionAndLossOfEligibilityAtSettlement() public {
        p.configureTab(address(tab));
        assertEq(p.effectiveFeeBps(alice), 0);
        _job();
        vm.startPrank(alice);
        tab.transfer(carol, tab.balanceOf(alice));
        p.submitJob(J, EVIDENCE);
        vm.stopPrank();
        vm.prank(bob); p.acceptJob(J, EVIDENCE);
        assertEq(p.getJob(J).feePaid, 0.5 ether);
    }
    function testHolderPaysZeroWorkFee() public {
        p.configureTab(address(tab));
        _job();
        vm.prank(alice); p.submitJob(J, EVIDENCE);
        vm.prank(bob); p.acceptJob(J, EVIDENCE);
        assertEq(p.getJob(J).feePaid, 0);
        assertEq(p.getJob(J).rewardPaid, 100 ether);
    }
}
