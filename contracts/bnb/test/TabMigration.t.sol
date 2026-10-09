// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {TabBase} from "./TabBNB.t.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabBacking} from "../src/TabBacking.sol";
import {TabEconomics} from "../src/TabEconomics.sol";
import {TabTypes as T} from "../src/TabTypes.sol";

contract TabMigrationTest is TabBase {
    TabProtocol next;
    function setUp() public override {
        super.setUp();
        next = TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(u), address(this))));
        next.configureLegacy(address(p));
        TabBacking backing = TabBacking(deployCode("TabBacking.sol:TabBacking", abi.encode(address(next))));
        TabEconomics economics = TabEconomics(deployCode("TabEconomics.sol:TabEconomics", abi.encode(address(next))));
        next.configureModules(address(backing), address(economics));
    }
    function _ids(bytes32 id) internal pure returns (bytes32[] memory values) {
        values = new bytes32[](1); values[0] = id;
    }
    function testImportPreservesIdentityPolicyVersionAndPauseWithoutFundsOrSessions() public {
        vm.startPrank(alice);
        p.updatePolicy(A, 50 ether, EVIDENCE);
        p.pauseAgent(A, true);
        p.fundSpending(A, 10 ether);
        vm.stopPrank();
        vm.prank(carol);
        next.importAgents(_ids(A));
        T.Agent memory previous = p.getAgent(A);
        T.Agent memory imported = next.getAgent(A);
        assertEq(keccak256(abi.encode(imported)), keccak256(abi.encode(previous)));
        assertEq(next.getSpending(A).available, 0);
        assertEq(p.getSpending(A).available, 10 ether);
        assertEq(next.totalLiability(), 0);
        vm.prank(carol);
        vm.expectRevert(TabProtocol.Authority.selector);
        next.updatePolicy(A, 1 ether, POLICY);
        vm.prank(alice);
        next.pauseAgent(A, false);
    }
    function testLegacyOwnerCannotResetPolicyByRegisteringAgain() public {
        vm.prank(alice);
        vm.expectRevert(TabProtocol.Terms.selector);
        next.register(A, "reset", 1 ether, POLICY);
        next.importAgents(_ids(A));
        vm.expectRevert(TabProtocol.Terms.selector);
        next.importAgents(_ids(A));
    }
    function testUnknownIdentityAndLegacyReplacementAreRejected() public {
        vm.expectRevert(TabProtocol.Terms.selector);
        next.importAgents(_ids(bytes32(uint256(3))));
        vm.expectRevert(TabProtocol.Authority.selector);
        next.configureLegacy(address(p));
    }
    function testNewRegistrationsStillBelongToTheirSigner() public {
        bytes32 id = bytes32(bytes.concat(bytes20(carol), bytes12(uint96(88))));
        vm.prank(carol);
        next.register(id, "new agent", 1 ether, POLICY);
        assertEq(next.getAgent(id).owner, carol);
    }
}
