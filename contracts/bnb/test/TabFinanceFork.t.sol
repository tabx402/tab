// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabTypes as T} from "../src/TabTypes.sol";
import {TabLendingPool} from "../src/TabLendingPool.sol";
import {TabPriceOracle} from "../src/TabPriceOracle.sol";
import {TabStockLending} from "../src/TabStockLending.sol";
import {TabBuyback} from "../src/TabBuyback.sol";
import {FinanceToken, FinanceFeed, FinanceMarket} from "./TabFinanceTestBase.sol";
import {FinanceRouter} from "./TabBuyback.t.sol";

/// @dev All authority impersonation, balance changes and deployments occur only in the local Foundry fork.
/// The default protocol is the immutable deployment recorded in contracts/deployments/bnb-56.json.
contract TabFinanceForkTest is Test {
    address constant USDT = 0x55d398326f99059fF775485246999027B3197955;
    address constant DEPLOYED_PROTOCOL = address(bytes20(hex"be2c140c0b40d25ef5531d93c0696318319e6375"));
    address constant BORROWER = address(0xB0B);
    address constant LENDER = address(0x1E);
    address constant BUYER = address(0xA11CE);
    address constant MERCHANT = address(0xCAFE);
    bytes32 constant A = bytes32((uint256(uint160(BORROWER)) << 96) | 938491);
    bytes32 constant JOB = keccak256("Tab new-module fork job");
    bytes32 constant LOAN = keccak256("Tab new-module fork loan");
    bytes32 constant H = keccak256("Tab new-module fork terms and receipts");
    TabProtocol protocol;
    IERC20 usdt;
    FinanceToken official;

    function setUp() public {
        string memory rpc = vm.envOr("BNB_FORK_RPC_URL", string(""));
        if (bytes(rpc).length == 0) {
            vm.skip(true);
            return;
        }
        vm.createSelectFork(rpc);
        assertEq(block.chainid, 56);
        protocol = TabProtocol(vm.envOr("BNB_LIVE_PROTOCOL", DEPLOYED_PROTOCOL));
        usdt = IERC20(USDT);
        assertEq(address(protocol.usdt()), USDT);
        assertTrue(address(protocol).code.length > 0);
        deal(USDT, LENDER, 10_000 ether);
        deal(USDT, BORROWER, 10_000 ether);
        deal(USDT, BUYER, 10_000 ether);
        // These candidate-module tests use a local holder token through the
        // protocol's external getter. They must not overwrite or reconfigure
        // the real protocol's one-time official TAB setting after activation.
        official = new FinanceToken(18);
        vm.mockCall(address(protocol), abi.encodeWithSignature("tabToken()"), abi.encode(address(official)));
        official.mint(LENDER, 1 ether);
        official.mint(BORROWER, 1 ether);
        official.mint(protocol.authority(), 1 ether);
    }

    function testDeployedProtocolWorkingCapitalInteropOnBnbFork() public {
        FinanceFeed bnbFeed = new FinanceFeed(8, 1000e8);
        FinanceFeed usdtFeed = new FinanceFeed(8, 1e8);
        TabPriceOracle oracle = new TabPriceOracle(0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c, USDT, address(bnbFeed), address(usdtFeed), 1 hours, 1 hours);
        TabLendingPool pool = new TabLendingPool(address(protocol), address(oracle));
        vm.deal(BORROWER, 1 ether);
        vm.startPrank(LENDER);
        usdt.approve(address(pool), 100 ether);
        pool.deposit(100 ether, LENDER);
        vm.stopPrank();
        address[] memory recipients = new address[](1);
        recipients[0] = MERCHANT;
        vm.prank(BORROWER);
        protocol.register(A, "new module fork", 100 ether, H);
        vm.startPrank(BUYER);
        usdt.approve(address(protocol), 50 ether);
        protocol.openJob(
            T.JobTerms(JOB, BORROWER, 50 ether, 1 ether, uint64(block.timestamp + 1 days), H, 1, recipients)
        );
        vm.stopPrank();
        vm.prank(BORROWER);
        protocol.bindJobAgent(JOB, A);
        vm.prank(protocol.authority());
        pool.approveLoan(
            TabLendingPool.LoanTerms(
                LOAN,
                A,
                JOB,
                BORROWER,
                10 ether,
                1 ether,
                10 ether,
                uint64(block.timestamp + 1 hours),
                1,
                recipients
            )
        );
        vm.startPrank(BORROWER);
        pool.acceptLoan(LOAN);
        pool.pledgeCollateral{value: 0.01 ether}(LOAN);
        pool.spendLoan(LOAN, MERCHANT, 1 ether, 1, H, H);
        usdt.approve(address(pool), 1 ether);
        pool.repayLoan(LOAN, 1 ether);
        pool.closeLoan(LOAN);
        pool.withdrawCollateral(LOAN, 0.01 ether);
        vm.stopPrank();
        vm.prank(LENDER);
        pool.redeem(100 ether, LENDER, LENDER);
        assertEq(pool.outstanding(), 0);
        assertEq(usdt.balanceOf(address(pool)), 0);
        assertEq(usdt.balanceOf(LENDER), 10_000 ether);
    }

    function testCanonicalUsdtCollateralPoolRepayAndWithdrawOnBnbFork() public {
        FinanceToken collateral = new FinanceToken(6);
        FinanceFeed stableFeed = new FinanceFeed(8, 1e8);
        FinanceFeed stockFeed = new FinanceFeed(8, 100e8);
        FinanceMarket market = new FinanceMarket();
        market.set(address(collateral), true);
        TabStockLending pool = new TabStockLending(address(protocol), address(stableFeed), 1 hours);
        vm.prank(protocol.authority());
        pool.configureCollateral(
            address(collateral), address(stockFeed), address(market), 1 hours, 5000, 7000, 500, 100 ether
        );
        vm.startPrank(LENDER);
        usdt.approve(address(pool), 100 ether);
        pool.deposit(100 ether, LENDER);
        vm.stopPrank();
        collateral.mint(BORROWER, 2e6);
        vm.startPrank(BORROWER);
        collateral.approve(address(pool), 2e6);
        pool.borrow(LOAN, address(collateral), 2e6, 100 ether);
        usdt.approve(address(pool), 100 ether);
        pool.repay(LOAN, 100 ether);
        pool.withdrawCollateral(LOAN, 2e6);
        vm.stopPrank();
        vm.prank(LENDER);
        pool.redeem(100 ether, LENDER, LENDER);
        assertEq(pool.collateralLiability(address(collateral)), 0);
        assertEq(usdt.balanceOf(address(pool)), 0);
        assertEq(collateral.balanceOf(BORROWER), 2e6);
    }

    function testCanonicalUsdtExplicitBuybackFundingOnBnbFork() public {
        // Official-token configuration is simulated solely in this isolated fork, never on mainnet.
        FinanceRouter router = new FinanceRouter();
        address[] memory route = new address[](2);
        route[0] = USDT;
        route[1] = address(official);
        TabBuyback buyback = new TabBuyback(address(protocol), address(router), route, 500);
        vm.startPrank(LENDER);
        usdt.approve(address(buyback), 10 ether);
        buyback.fund(10 ether);
        vm.stopPrank();
        vm.prank(protocol.authority());
        buyback.execute(10 ether, 19 ether, block.timestamp + 5 minutes);
        assertEq(buyback.spentUsdt(), 10 ether);
        assertEq(official.balanceOf(buyback.DEAD()), 20 ether);
        assertEq(usdt.allowance(address(buyback), address(router)), 0);
    }
}
