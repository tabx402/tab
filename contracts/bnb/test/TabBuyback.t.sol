// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {FinanceBase, FinanceToken, FinanceTaxToken} from "./TabFinanceTestBase.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {TabBuyback} from "../src/TabBuyback.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabHolderAccess} from "../src/TabHolderAccess.sol";

contract FinanceRouter {
    uint256 public quotedRate = 2;
    uint256 public deliveredRate = 2;
    uint8 public mode;
    uint256 public lastMinimum;
    uint256 public lastDeadline;

    function configure(uint256 quoted, uint256 delivered, uint8 mode_) external {
        quotedRate = quoted;
        deliveredRate = delivered;
        mode = mode_;
    }

    function getAmountsOut(uint256 input, address[] calldata path)
        external
        view
        returns (uint256[] memory values)
    {
        values = new uint256[](path.length);
        values[0] = mode == 1 ? input + 1 : input;
        values[path.length - 1] = input * quotedRate;
    }

    function swapExactTokensForTokens(
        uint256 input,
        uint256 minimum,
        address[] calldata path,
        address to,
        uint256 deadline
    ) external returns (uint256[] memory values) {
        require(deadline >= block.timestamp);
        lastMinimum = minimum;
        lastDeadline = deadline;
        uint256 received = input * deliveredRate;
        require(received >= minimum || mode == 2);
        IERC20(path[0]).transferFrom(msg.sender, address(this), mode == 3 ? input - 1 : input);
        FinanceToken(path[path.length - 1]).mint(to, received);
        values = new uint256[](path.length);
        values[0] = input;
        values[path.length - 1] = mode == 4 ? received + 1 : received;
    }
}

contract TabBuybackTest is FinanceBase {
    FinanceToken token;
    FinanceRouter router;
    TabBuyback buyback;

    function setUp() public override {
        super.setUp();
        token = new FinanceToken(18);
        token.mint(lender, 1 ether);
        token.mint(address(this), 1 ether);
        protocol.configureTab(address(token));
        router = new FinanceRouter();
        buyback = new TabBuyback(address(protocol), address(router), _route(), 500);
        _approve(address(usdt), lender, address(buyback));
    }

    function _route() internal view returns (address[] memory route) {
        route = new address[](2);
        route[0] = address(usdt);
        route[1] = address(token);
    }

    function _fund() internal {
        vm.prank(lender);
        buyback.fund(100 ether);
    }

    function testBuybackFundingAndExecutionRequireCurrentHoldings() public {
        _fund();
        vm.prank(lender);
        token.transfer(merchant, 1 ether);
        vm.prank(lender);
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        buyback.fund(1 ether);
        token.transfer(merchant, token.balanceOf(address(this)));
        vm.expectRevert(TabHolderAccess.TabHoldingRequired.selector);
        buyback.execute(1 ether, 1 ether, block.timestamp + 1 minutes);
        assertEq(buyback.spentUsdt(), 0);
        buyback.setPaused(true);
        assertTrue(buyback.paused());
    }

    function testExplicitFundingPurchaseAndDeadAddressReceipt() public {
        _fund();
        (uint256 quote, uint256 minimum) = buyback.quote(10 ether);
        assertEq(quote, 20 ether);
        assertEq(minimum, 19 ether);
        buyback.execute(10 ether, minimum, block.timestamp + 5 minutes);
        assertEq(buyback.fundedUsdt(), 100 ether);
        assertEq(buyback.spentUsdt(), 10 ether);
        assertEq(buyback.availableUsdt(), 90 ether);
        assertEq(buyback.tokensAcquired(), 20 ether);
        assertEq(token.balanceOf(buyback.DEAD()), 20 ether);
        assertEq(usdt.balanceOf(address(buyback)), 90 ether);
        assertEq(usdt.balanceOf(address(router)), 10 ether);
        assertEq(usdt.allowance(address(buyback), address(router)), 0);
        assertEq(token.balanceOf(address(buyback)), 0);
    }

    function testFundingCannotBeInferredFromDirectDonation() public {
        usdt.mint(address(buyback), 100 ether);
        assertEq(buyback.availableUsdt(), 0);
        vm.expectRevert(TabBuyback.Terms.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
    }

    function testOnlyOperatorCanExecuteOrPause() public {
        _fund();
        vm.prank(lender);
        vm.expectRevert(TabBuyback.Authority.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        vm.prank(lender);
        vm.expectRevert(TabBuyback.Authority.selector);
        buyback.setPaused(true);
    }

    function testSlippageFloorAndBoundedDeadline() public {
        _fund();
        vm.expectRevert(TabBuyback.Terms.selector);
        buyback.execute(10 ether, 19 ether - 1, block.timestamp + 5 minutes);
        vm.expectRevert(TabBuyback.Terms.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp - 1);
        vm.expectRevert(TabBuyback.Terms.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 10 minutes + 1);
        vm.expectRevert(TabBuyback.Terms.selector);
        buyback.execute(101 ether, 191.9 ether, block.timestamp + 5 minutes);
    }

    function testPausePreventsSwapAndDoesNotGrantAssetWithdrawal() public {
        _fund();
        buyback.setPaused(true);
        vm.expectRevert(TabBuyback.Terms.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        assertEq(buyback.availableUsdt(), 100 ether);
    }

    function testUnconfiguredOfficialTokenCannotDeployBuyback() public {
        TabProtocol empty =
            TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(usdt), address(this))));
        vm.expectRevert(TabBuyback.Terms.selector);
        new TabBuyback(address(empty), address(router), _route(), 500);
    }

    function testRouteCannotTargetOtherTokenOrRepeatAssets() public {
        address[] memory wrong = _route();
        wrong[1] = address(new FinanceToken(18));
        vm.expectRevert(TabBuyback.Terms.selector);
        new TabBuyback(address(protocol), address(router), wrong, 500);
        address[] memory repeat = new address[](3);
        repeat[0] = address(usdt);
        repeat[1] = address(usdt);
        repeat[2] = address(token);
        vm.expectRevert(TabBuyback.Terms.selector);
        new TabBuyback(address(protocol), address(router), repeat, 500);
    }

    function testBadQuoteAndActualOutputMismatchRevertAllFundingMovement() public {
        _fund();
        router.configure(2, 2, 1);
        vm.expectRevert(TabBuyback.Terms.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        router.configure(2, 1, 2);
        vm.expectRevert(TabBuyback.TransferFailed.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        router.configure(2, 2, 4);
        vm.expectRevert(TabBuyback.TransferFailed.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        assertEq(usdt.balanceOf(address(buyback)), 100 ether);
        assertEq(buyback.spentUsdt(), 0);
        assertEq(token.balanceOf(buyback.DEAD()), 0);
        assertEq(usdt.allowance(address(buyback), address(router)), 0);
    }

    function testRouterMustSpendExactApprovedUSDT() public {
        _fund();
        router.configure(2, 2, 3);
        vm.expectRevert(TabBuyback.TransferFailed.selector);
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        assertEq(usdt.balanceOf(address(buyback)), 100 ether);
    }

    function testOldProtocolFeesAreNotWithdrawnOrCountedAsBuybackFunding() public {
        _job();
        vm.prank(borrower);
        protocol.submitJob(JOB, RECEIPT);
        vm.prank(buyer);
        protocol.acceptJob(JOB, RECEIPT);
        uint256 expectedFee = uint256(100 ether) * protocol.feeBps() / 10_000;
        assertEq(protocol.feesCollectedUsdt(), expectedFee);
        assertEq(usdt.balanceOf(address(protocol)), expectedFee);
        assertEq(buyback.fundedUsdt(), 0);
        _fund();
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        assertEq(usdt.balanceOf(address(protocol)), expectedFee);
        assertEq(protocol.getProtocol().buybackSpentUsdt, 0);
        assertEq(buyback.spentUsdt(), 10 ether);
    }

    function testFeeOnTransferOfficialTokenCannotPretendFullReceipt() public {
        FinanceTaxToken taxed = new FinanceTaxToken(18);
        TabProtocol local =
            TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(usdt), address(this))));
        local.configureTab(address(taxed));
        taxed.mint(lender, 1 ether);
        taxed.mint(address(this), 1 ether);
        address[] memory route = new address[](2);
        route[0] = address(usdt);
        route[1] = address(taxed);
        TabBuyback localBuyback = new TabBuyback(address(local), address(router), route, 500);
        _approve(address(usdt), lender, address(localBuyback));
        vm.prank(lender);
        localBuyback.fund(100 ether);
        taxed.setTax(true);
        vm.expectRevert(TabBuyback.TransferFailed.selector);
        localBuyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        assertEq(localBuyback.spentUsdt(), 0);
        assertEq(usdt.balanceOf(address(localBuyback)), 100 ether);
    }

    function testFuzzFundingSpendConservation(uint128 fundingSeed, uint128 spendingSeed) public {
        uint256 funding = bound(fundingSeed, 1, 1_000 ether);
        uint256 spending = bound(spendingSeed, 1, funding);
        vm.prank(lender);
        buyback.fund(funding);
        buyback.execute(spending, spending * 2, block.timestamp + 5 minutes);
        assertEq(buyback.spentUsdt() + buyback.availableUsdt(), funding);
        assertEq(usdt.balanceOf(address(buyback)), buyback.availableUsdt());
        assertEq(token.balanceOf(buyback.DEAD()), spending * 2);
    }
}
