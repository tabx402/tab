// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Test} from "forge-std/Test.sol";
import {IERC1271} from "@openzeppelin/contracts/interfaces/IERC1271.sol";
import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {MockToken} from "./TabBNB.t.sol";

contract RegistrationWallet is IERC1271 {
    address private immutable signer;
    bool public revoked;

    constructor(address signer_) {
        signer = signer_;
    }

    function revoke() external {
        revoked = true;
    }

    function isValidSignature(bytes32 hash, bytes memory signature) external view returns (bytes4) {
        return !revoked && ECDSA.recover(hash, signature) == signer
            ? IERC1271.isValidSignature.selector
            : bytes4(0xffffffff);
    }
}

/// Registration uses no allowance, transfer, or agent-owner balance.
contract TabRegistrationTest is Test {
    uint256 constant OWNER_KEY = 0xAABBCC;
    bytes32 constant POLICY = keccak256("registration policy");
    TabProtocol protocol;
    address owner;
    address relayer = address(0xB0B);
    uint256 deadline;

    function setUp() public {
        vm.chainId(31337);
        vm.warp(10 days);
        owner = vm.addr(OWNER_KEY);
        MockToken token = new MockToken(18);
        protocol =
            TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(token), address(this))));
        deadline = block.timestamp + 300;
    }

    function _id(address account, uint96 suffix) internal pure returns (bytes32) {
        return bytes32((uint256(uint160(account)) << 96) | suffix);
    }

    function _signature(
        address account,
        bytes32 id,
        uint256 nonce,
        uint256 until,
        string memory domainName,
        string memory version,
        uint256 chain,
        address target
    ) internal view returns (bytes memory) {
        bytes32 domain = keccak256(
            abi.encode(
                keccak256(
                    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                ),
                keccak256(bytes(domainName)),
                keccak256(bytes(version)),
                chain,
                target
            )
        );
        bytes32 payload = keccak256(
            abi.encode(
                protocol.REGISTER_TYPEHASH(),
                id,
                account,
                keccak256("sponsored agent"),
                1 ether,
                POLICY,
                nonce,
                until
            )
        );
        (uint8 v, bytes32 r, bytes32 s) =
            vm.sign(OWNER_KEY, keccak256(abi.encodePacked("\x19\x01", domain, payload)));
        return abi.encodePacked(r, s, v);
    }

    function _signed(address account, bytes32 id, uint256 nonce, uint256 until)
        internal
        view
        returns (bytes memory)
    {
        return _signature(account, id, nonce, until, "Tab Protocol", "1", 31337, address(protocol));
    }

    function _register(address account, bytes32 id, uint256 nonce, uint256 until, bytes memory signature)
        internal
    {
        protocol.registerWithSignature(
            id, account, "sponsored agent", 1 ether, POLICY, nonce, until, signature
        );
    }

    function testUntrustedRelayerCannotTakeOwnershipOfZeroBalanceOwner() public {
        bytes32 id = _id(owner, 1);
        bytes memory signature = _signed(owner, id, 0, deadline);
        assertEq(owner.balance, 0);
        vm.prank(relayer);
        _register(owner, id, 0, deadline, signature);
        assertEq(protocol.getAgent(id).owner, owner);
        assertEq(owner.balance, 0);
        assertEq(protocol.registrationNonces(owner), 1);
        vm.prank(relayer);
        vm.expectRevert(TabProtocol.Authority.selector);
        protocol.pauseAgent(id, true);
        vm.prank(owner);
        protocol.pauseAgent(id, true);
        assertTrue(protocol.getAgent(id).paused);
    }

    function testErc1271OwnerIsWalletNotItsSignerOrRelayer() public {
        RegistrationWallet wallet = new RegistrationWallet(owner);
        bytes32 id = _id(address(wallet), 1);
        vm.prank(relayer);
        _register(address(wallet), id, 0, deadline, _signed(address(wallet), id, 0, deadline));
        assertEq(protocol.getAgent(id).owner, address(wallet));
        assertEq(protocol.registrationNonces(address(wallet)), 1);
        assertEq(protocol.registrationNonces(owner), 0);
    }

    function testRevokedErc1271SignatureCannotRegisterOrConsumeNonce() public {
        RegistrationWallet wallet = new RegistrationWallet(owner);
        bytes32 id = _id(address(wallet), 1);
        bytes memory signature = _signed(address(wallet), id, 0, deadline);
        wallet.revoke();
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(address(wallet), id, 0, deadline, signature);
        assertEq(protocol.registrationNonces(address(wallet)), 0);
    }

    function testEveryDomainFieldIsBound() public {
        bytes32 id = _id(owner, 1);
        bytes[] memory bad = new bytes[](4);
        bad[0] = _signature(owner, id, 0, deadline, "Other Protocol", "1", 31337, address(protocol));
        bad[1] = _signature(owner, id, 0, deadline, "Tab Protocol", "2", 31337, address(protocol));
        bad[2] = _signature(owner, id, 0, deadline, "Tab Protocol", "1", 56, address(protocol));
        bad[3] = _signature(owner, id, 0, deadline, "Tab Protocol", "1", 31337, relayer);
        for (uint256 i; i < bad.length; i++) {
            vm.expectRevert(TabProtocol.Authority.selector);
            _register(owner, id, 0, deadline, bad[i]);
            assertEq(protocol.registrationNonces(owner), 0);
        }
    }

    function testSignedPayloadCannotBeMutated() public {
        bytes32 id = _id(owner, 1);
        bytes memory signature = _signed(owner, id, 0, deadline);
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(owner, _id(owner, 2), 0, deadline, signature);
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(relayer, id, 0, deadline, signature);
        vm.expectRevert(TabProtocol.Authority.selector);
        protocol.registerWithSignature(id, owner, "different name", 1 ether, POLICY, 0, deadline, signature);
        vm.expectRevert(TabProtocol.Authority.selector);
        protocol.registerWithSignature(id, owner, "sponsored agent", 2 ether, POLICY, 0, deadline, signature);
        vm.expectRevert(TabProtocol.Authority.selector);
        protocol.registerWithSignature(
            id, owner, "sponsored agent", 1 ether, bytes32(uint256(1)), 0, deadline, signature
        );
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(owner, id, 1, deadline, signature);
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(owner, id, 0, deadline + 1, signature);
        assertEq(protocol.registrationNonces(owner), 0);
        _register(owner, id, 0, deadline, signature);
    }

    function testConcurrentOwnerRegistrationsRequireNextNonce() public {
        bytes32 first = _id(owner, 1);
        bytes32 second = _id(owner, 2);
        bytes memory secondStale = _signed(owner, second, 0, deadline);
        _register(owner, first, 0, deadline, _signed(owner, first, 0, deadline));
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(owner, second, 0, deadline, secondStale);
        assertEq(protocol.registrationNonces(owner), 1);
        _register(owner, second, 1, deadline, _signed(owner, second, 1, deadline));
        assertEq(protocol.registrationNonces(owner), 2);
    }

    function testExpiredAndFutureNonceDoNotConsumeAuthorization() public {
        bytes32 id = _id(owner, 1);
        bytes memory expired = _signed(owner, id, 0, block.timestamp - 1);
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(owner, id, 0, block.timestamp - 1, expired);
        bytes memory future = _signed(owner, id, 1, deadline);
        vm.expectRevert(TabProtocol.Authority.selector);
        _register(owner, id, 1, deadline, future);
        assertEq(protocol.registrationNonces(owner), 0);
    }

    function testEvenValidSignatureCannotRegisterOtherOwnersNamespace() public {
        bytes32 id = _id(relayer, 1);
        bytes memory signature = _signed(owner, id, 0, deadline);
        vm.expectRevert(TabProtocol.Terms.selector);
        _register(owner, id, 0, deadline, signature);
        assertEq(protocol.registrationNonces(owner), 0);
    }
}
