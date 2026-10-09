// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {FinanceBase, FinanceToken} from "./TabFinanceTestBase.sol";
import {TabMarketHours} from "../src/TabMarketHours.sol";

contract TabMarketHoursTest is FinanceBase {
    TabMarketHours market;
    FinanceToken collateral;

    function setUp() public override {
        super.setUp();
        market = new TabMarketHours(address(this));
        collateral = new FinanceToken(6);
    }

    function testDefaultClosedAndExpiredAttestationCloses() public {
        assertFalse(market.isOpen(address(collateral)));
        market.publish(address(collateral), true, uint64(block.timestamp + 5 minutes), RECEIPT);
        assertTrue(market.isOpen(address(collateral)));
        vm.warp(block.timestamp + 5 minutes);
        assertFalse(market.isOpen(address(collateral)));
    }

    function testClosingRequiresNoFutureValidityAndIsImmediate() public {
        market.publish(address(collateral), true, uint64(block.timestamp + 5 minutes), RECEIPT);
        market.publish(address(collateral), false, 0, RECEIPT);
        assertFalse(market.isOpen(address(collateral)));
        (bool open, uint64 validUntil,, bytes32 source) = market.statuses(address(collateral));
        assertFalse(open);
        assertEq(validUntil, block.timestamp);
        assertEq(source, RECEIPT);
    }

    function testAuthorityAndShortValidityAreRequired() public {
        vm.prank(borrower);
        vm.expectRevert(TabMarketHours.Authority.selector);
        market.publish(address(collateral), true, uint64(block.timestamp + 5 minutes), RECEIPT);
        vm.expectRevert(TabMarketHours.Terms.selector);
        market.publish(address(collateral), true, uint64(block.timestamp + 15 minutes + 1), RECEIPT);
        vm.expectRevert(TabMarketHours.Terms.selector);
        market.publish(address(collateral), true, uint64(block.timestamp), RECEIPT);
        vm.expectRevert(TabMarketHours.Terms.selector);
        market.publish(address(collateral), true, uint64(block.timestamp + 5 minutes), 0);
    }
}
