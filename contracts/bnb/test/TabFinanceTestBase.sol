// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;
import {Test} from "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {TabProtocol} from "../src/TabProtocol.sol";
import {TabTypes as T} from "../src/TabTypes.sol";

contract FinanceToken is ERC20 {
    uint8 private precision;

    constructor(uint8 d) ERC20("Finance test token", "FIN") {
        precision = d;
    }

    function decimals() public view override returns (uint8) {
        return precision;
    }

    function mint(address recipient, uint256 amount) external {
        _mint(recipient, amount);
    }
}

contract FinanceTaxToken is FinanceToken {
    bool public tax;
    constructor(uint8 d) FinanceToken(d) {}

    function setTax(bool value) external {
        tax = value;
    }

    function _update(address from, address to, uint256 amount) internal override {
        if (tax && from != address(0) && to != address(0)) {
            uint256 fee = amount / 100;
            super._update(from, address(0), fee);
            amount -= fee;
        }
        super._update(from, to, amount);
    }
}

contract FinanceCallbackToken is FinanceToken {
    address public target;
    bool public blocked;
    bytes4 public blockedError;
    constructor() FinanceToken(18) {}

    function arm(address target_) external {
        target = target_;
    }

    function _update(address from, address to, uint256 amount) internal override {
        super._update(from, to, amount);
        if (from != address(0) && to == target) {
            address callback = target;
            target = address(0);
            (bool ok, bytes memory result) =
                callback.call(abi.encodeWithSignature("deposit(uint256,address)", 1, address(this)));
            blocked = !ok;
            if (result.length >= 4) blockedError = bytes4(result);
        }
    }
}

contract FinanceFeed {
    uint8 public immutable decimals;
    int256 public answer;
    uint80 public roundId = 1;
    uint80 public answeredInRound = 1;
    uint256 public updatedAt;

    constructor(uint8 precision, int256 value) {
        decimals = precision;
        answer = value;
        updatedAt = block.timestamp;
    }

    function set(int256 value) external {
        answer = value;
        updatedAt = block.timestamp;
        roundId++;
        answeredInRound = roundId;
    }

    function setRound(uint80 round, uint80 answered, uint256 updated) external {
        roundId = round;
        answeredInRound = answered;
        updatedAt = updated;
    }

    function latestRoundData() external view returns (uint80, int256, uint256, uint256, uint80) {
        return (roundId, answer, updatedAt, updatedAt, answeredInRound);
    }
}

contract FinanceMarket {
    mapping(address => bool) public open;

    function set(address token, bool value) external {
        open[token] = value;
    }

    function isOpen(address token) external view returns (bool) {
        return open[token];
    }
}

abstract contract FinanceBase is Test {
    FinanceToken internal usdt;
    FinanceToken internal holderToken;
    TabProtocol internal protocol;
    address internal lender = address(0x1E);
    address internal borrower = address(0xB0B);
    address internal buyer = address(0xA11CE);
    address internal merchant = address(0xCAFE);
    address internal signer = address(0x51);
    bytes32 internal constant AGENT = bytes32((uint256(uint160(address(0xB0B))) << 96) | 1);
    bytes32 internal constant JOB = keccak256("finance-job");
    bytes32 internal constant LOAN = keccak256("finance-loan");
    bytes32 internal constant RECEIPT = keccak256("finance-receipt");

    function setUp() public virtual {
        vm.chainId(31337);
        vm.warp(10 days);
        usdt = new FinanceToken(18);
        protocol =
            TabProtocol(deployCode("TabProtocol.sol:TabProtocol", abi.encode(address(usdt), address(this))));
        usdt.mint(lender, 10_000 ether);
        usdt.mint(borrower, 10_000 ether);
        usdt.mint(buyer, 10_000 ether);
        usdt.mint(merchant, 10_000 ether);
        vm.prank(borrower);
        protocol.register(AGENT, "finance bird", 100 ether, keccak256("policy"));
    }

    function _approve(address token, address actor, address target) internal {
        vm.prank(actor);
        FinanceToken(token).approve(target, type(uint256).max);
    }

    function _holders(TabProtocol target) internal {
        if (address(holderToken) == address(0)) {
            holderToken = new FinanceToken(18);
            holderToken.mint(lender, 1 ether);
            holderToken.mint(borrower, 1 ether);
            holderToken.mint(address(this), 1 ether);
        }
        target.configureTab(address(holderToken));
    }

    function _job() internal {
        address[] memory recipients = new address[](1);
        recipients[0] = merchant;
        _approve(address(usdt), buyer, address(protocol));
        vm.prank(buyer);
        protocol.openJob(
            T.JobTerms(
                JOB,
                borrower,
                100 ether,
                10 ether,
                uint64(block.timestamp + 2 days),
                keccak256("terms"),
                7,
                recipients
            )
        );
        vm.prank(borrower);
        protocol.bindJobAgent(JOB, AGENT);
    }
}
