// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {Test} from "forge-std/Test.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabBacking} from "../src/TabBacking.sol";
import {TabEconomics} from "../src/TabEconomics.sol";
import {TabTypes as T} from "../src/TabTypes.sol";

/// @dev Optional read-only RPC fork. All balance overrides and transactions are local to Foundry.
contract TabBNBForkTest is Test {
    address constant USDT = 0x55d398326f99059fF775485246999027B3197955;
    address constant ALICE = address(0xA11CE);
    address constant BUYER = address(0xB0B);
    address constant MERCHANT = address(0xCAFE);
    bytes32 constant A = bytes32((uint256(uint160(ALICE)) << 96) | 1);
    bytes32 constant H = keccak256("terms");
    TabProtocol p;
    TabBacking b;
    TabEconomics e;
    IERC20 u;

    function setUp() public {
        string memory rpc = vm.envOr("BNB_FORK_RPC_URL", string(""));
        if (bytes(rpc).length == 0) {
            vm.skip(true);
            return;
        }
        vm.createSelectFork(rpc);
        assertEq(block.chainid, 56);
        assertEq(IERC20Metadata(USDT).decimals(), 18);
        u = IERC20(USDT);
        p = TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(USDT, address(this))));
        b = TabBacking(deployCode("TabBacking.sol:TabBacking", abi.encode(address(p))));
        e = TabEconomics(deployCode("TabEconomics.sol:TabEconomics", abi.encode(address(p))));
        p.configureModules(address(b), address(e));
        deal(USDT, ALICE, 100 ether);
        deal(USDT, BUYER, 100 ether);
        vm.startPrank(ALICE);
        u.approve(address(p), 100 ether);
        u.approve(address(b), 100 ether);
        p.register(A, "BNB fork agent", 100 ether, H);
        vm.stopPrank();
        vm.startPrank(BUYER);
        u.approve(address(p), 100 ether);
        u.approve(address(b), 100 ether);
        vm.stopPrank();
    }

    function _providers() internal pure returns (address[] memory recipients) {
        recipients = new address[](1);
        recipients[0] = MERCHANT;
    }

    function testCanonicalUsdtJobSettlementOnBnbFork() public {
        vm.prank(BUYER);
        p.openJob(
            T.JobTerms(H, ALICE, 10 ether, 1 ether, uint64(block.timestamp + 1 days), H, 1, _providers())
        );
        vm.startPrank(ALICE);
        p.bindJobAgent(H, A);
        p.payCall(H, MERCHANT, 1 ether, 1, H, H);
        p.submitJob(H, H);
        vm.stopPrank();
        vm.prank(BUYER);
        p.acceptJob(H, H);
        assertEq(u.balanceOf(address(p)), 0.045 ether);
        assertEq(p.getJob(H).rewardPaid, 8.955 ether);
        assertEq(p.totalLiability(), 0.045 ether);
    }

    function testCanonicalUsdtCreditFundingRepaymentAndWithdrawalOnBnbFork() public {
        address[] memory recipients = new address[](1);
        recipients[0] = MERCHANT;
        vm.prank(BUYER);
        b.openCredit(
            T.CreditTerms(
                H, A, ALICE, 10 ether, 1 ether, 10 ether, uint64(block.timestamp + 1 days), 1, recipients
            )
        );
        vm.startPrank(ALICE);
        b.pledgeCollateral(H, 2 ether);
        b.acceptCredit(H);
        b.spendCredit(H, MERCHANT, 1 ether, 1, H, H);
        b.repayCredit(H, 1 ether);
        b.withdrawCollateral(H, 2 ether);
        vm.stopPrank();
        vm.prank(BUYER);
        b.withdrawCredit(H, 10 ether);
        assertEq(u.balanceOf(address(b)), 0);
        assertEq(b.getCredit(H).outstanding, 0);
        assertEq(u.balanceOf(BUYER), 100 ether);
    }

    function testCanonicalUsdtBackingAndBudgetCustodyOnBnbFork() public {
        vm.startPrank(ALICE);
        b.backAgent(A, USDT, 2 ether);
        b.withdrawBacking(A, USDT, 2 ether);
        p.fundSpending(A, 2 ether);
        p.withdrawSpending(A, 2 ether);
        vm.stopPrank();
        assertEq(u.balanceOf(ALICE), 100 ether);
        assertEq(p.totalLiability(), 0);
        assertEq(b.tokenLiability(USDT), 0);
    }
}
