// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {IERC20Metadata} from "@openzeppelin/contracts/token/ERC20/extensions/IERC20Metadata.sol";
import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";
import {TabUSDTLiquidity} from "./TabUSDTLiquidity.sol";

interface ITabPriceFeed {
    function decimals() external view returns (uint8);
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}

/// @notice Market-hours oracle required for listed/tokenized equities. Must cover the specific collateral.
interface ITabMarketStatus {
    function isOpen(address collateral) external view returns (bool);
}

/// @notice Whitelisted BEP20 collateral lending with explicit feed and market-hours gates.
/// @dev Collateral/USD is divided by fresh USDT/USD. No feed, whitelist or operating market is implied by deployment.
/// Every loan snapshots risk terms. There is no administrator collateral withdrawal or rehypothecation.
contract TabStockLending is TabUSDTLiquidity {
    uint256 public constant BPS = 10_000;
    ITabPriceFeed public immutable usdtUsdFeed;
    uint8 public immutable usdtFeedDecimals;
    uint32 public immutable usdtMaxAge;

    struct CollateralConfig {
        address feed;
        address marketStatus;
        uint32 maxAge;
        uint16 borrowLtvBps;
        uint16 liquidationLtvBps;
        uint16 liquidationBonusBps;
        uint8 tokenDecimals;
        uint8 feedDecimals;
        uint256 maxDebt;
        bool enabled;
    }

    struct Loan {
        address borrower;
        address collateralToken;
        uint256 collateral;
        uint256 debt;
        uint256 loss;
        CollateralConfig terms;
    }
    mapping(address => CollateralConfig) private configs;
    mapping(bytes32 => Loan) private loans;
    mapping(address => uint256) public collateralLiability;

    error PriceUnavailable();
    error MarketClosed();
    event CollateralConfigured(
        address indexed token,
        address feed,
        address marketStatus,
        uint16 borrowLtvBps,
        uint16 liquidationLtvBps
    );
    event CollateralDisabled(address indexed token);
    event StockLoanOpened(
        bytes32 indexed loan,
        address indexed borrower,
        address indexed token,
        uint256 collateral,
        uint256 principal
    );
    event CollateralAdded(bytes32 indexed loan, uint256 amount);
    event CollateralWithdrawn(bytes32 indexed loan, uint256 amount);
    event StockLoanRepaid(bytes32 indexed loan, address indexed payer, uint256 principal);
    event StockLoanLiquidated(
        bytes32 indexed loan, address indexed liquidator, uint256 repaid, uint256 collateralSeized
    );

    constructor(address protocol_, address usdtUsdFeed_, uint32 usdtMaxAge_)
        TabUSDTLiquidity(protocol_, "Tab collateral USDT", "tabCOL")
    {
        if (usdtUsdFeed_.code.length == 0 || usdtMaxAge_ < 60 || usdtMaxAge_ > 1 days) revert Terms();
        usdtUsdFeed = ITabPriceFeed(usdtUsdFeed_);
        usdtFeedDecimals = ITabPriceFeed(usdtUsdFeed_).decimals();
        if (usdtFeedDecimals > 18) revert Terms();
        usdtMaxAge = usdtMaxAge_;
        _feedPrice(usdtUsdFeed_, usdtMaxAge_);
    }

    function getCollateralConfig(address token) external view returns (CollateralConfig memory) {
        return configs[token];
    }

    function getAsset(address token) external view returns (CollateralConfig memory) {
        return configs[token];
    }

    function getLoan(bytes32 id) external view returns (Loan memory) {
        return loans[id];
    }

    function marketOpen(address token) external view returns (bool) {
        CollateralConfig memory c = configs[token];
        if (!c.enabled) return false;
        return ITabMarketStatus(c.marketStatus).isOpen(token);
    }

    function collateralQuote(address token, uint256 amount)
        external
        view
        returns (uint256 value, uint256 maximumBorrow, uint256 liquidationDebt)
    {
        CollateralConfig memory c = configs[token];
        if (!c.enabled || amount == 0) revert Terms();
        value = _value(amount, _price(c, token), c);
        maximumBorrow = Math.min(Math.mulDiv(value, c.borrowLtvBps, BPS), c.maxDebt);
        liquidationDebt = Math.mulDiv(value, c.liquidationLtvBps, BPS);
    }

    /// Whitelisting authorizes fresh loans only. Existing borrowers retain their original risk terms.
    function configureCollateral(
        address token,
        address feed,
        address marketStatus,
        uint32 maxAge,
        uint16 borrowLtvBps,
        uint16 liquidationLtvBps,
        uint16 liquidationBonusBps,
        uint256 maxDebt
    ) external {
        if (msg.sender != underwriter) revert Authority();
        if (
            token.code.length == 0 || token == address(usdt) || token == address(this)
                || feed.code.length == 0 || marketStatus.code.length == 0 || maxAge < 60 || maxAge > 1 days
                || borrowLtvBps == 0 || borrowLtvBps >= liquidationLtvBps || liquidationLtvBps > 9000
                || liquidationBonusBps > 1500
                || uint256(liquidationLtvBps) * (BPS + liquidationBonusBps) >= BPS * BPS || maxDebt == 0
                || maxDebt > protocol.MAX_BUDGET()
        ) revert Terms();
        uint8 td = IERC20Metadata(token).decimals();
        uint8 fd = ITabPriceFeed(feed).decimals();
        if (td > 18 || fd > 18) revert Terms();
        configs[token] = CollateralConfig(
            feed,
            marketStatus,
            maxAge,
            borrowLtvBps,
            liquidationLtvBps,
            liquidationBonusBps,
            td,
            fd,
            maxDebt,
            true
        );
        // Invalid/closed markets cannot silently be enabled for credit.
        _price(configs[token], token);
        emit CollateralConfigured(token, feed, marketStatus, borrowLtvBps, liquidationLtvBps);
    }

    function disableCollateral(address token) external {
        if (msg.sender != underwriter) revert Authority();
        configs[token].enabled = false;
        emit CollateralDisabled(token);
    }

    function _price(CollateralConfig memory c, address token) internal view returns (uint256) {
        if (!ITabMarketStatus(c.marketStatus).isOpen(token)) revert MarketClosed();
        if (ITabPriceFeed(c.feed).decimals() != c.feedDecimals) revert PriceUnavailable();
        return _feedPrice(c.feed, c.maxAge);
    }

    function _feedPrice(address feed, uint32 maxAge) internal view returns (uint256) {
        (uint80 round, int256 answer,, uint256 updated, uint80 answered) =
            ITabPriceFeed(feed).latestRoundData();
        if (
            answer <= 0 || uint256(answer) > 1e30 || round == 0 || answered < round || updated == 0
                || updated > block.timestamp || block.timestamp - updated > maxAge
        ) revert PriceUnavailable();
        return uint256(answer);
    }

    function _value(uint256 amount, uint256 price, CollateralConfig memory c)
        internal
        view
        returns (uint256)
    {
        if (usdtUsdFeed.decimals() != usdtFeedDecimals) revert PriceUnavailable();
        uint256 usdtPrice = _feedPrice(address(usdtUsdFeed), usdtMaxAge);
        return Math.mulDiv(
            amount,
            price * (10 ** (18 + uint256(usdtFeedDecimals))),
            usdtPrice * (10 ** (uint256(c.tokenDecimals) + c.feedDecimals))
        );
    }

    function borrow(bytes32 id, address token, uint256 collateralAmount, uint256 principal)
        external
        nonReentrant
    {
        CollateralConfig memory c = configs[token];
        if (
            paused || !c.enabled || id == 0 || loans[id].borrower != address(0) || collateralAmount == 0
                || principal == 0 || principal > c.maxDebt || principal > availableLiquidity()
        ) revert Terms();
        uint256 value = _value(collateralAmount, _price(c, token), c);
        if (principal > Math.mulDiv(value, c.borrowLtvBps, BPS)) revert Terms();
        loans[id] = Loan(msg.sender, token, collateralAmount, principal, 0, c);
        collateralLiability[token] += collateralAmount;
        _pull(IERC20(token), msg.sender, collateralAmount);
        _lend(msg.sender, principal);
        emit StockLoanOpened(id, msg.sender, token, collateralAmount, principal);
    }

    function addCollateral(bytes32 id, uint256 amount) external nonReentrant {
        Loan storage l = loans[id];
        if (l.borrower == address(0) || l.debt == 0 || amount == 0) revert Terms();
        l.collateral += amount;
        collateralLiability[l.collateralToken] += amount;
        _pull(IERC20(l.collateralToken), msg.sender, amount);
        emit CollateralAdded(id, amount);
    }

    function withdrawCollateral(bytes32 id, uint256 amount) external nonReentrant {
        Loan storage l = loans[id];
        if (msg.sender != l.borrower) revert Authority();
        if (amount == 0 || amount > l.collateral) revert Terms();
        uint256 remaining = l.collateral - amount;
        if (l.debt != 0) {
            if (paused) revert Terms();
            uint256 value = _value(remaining, _price(l.terms, l.collateralToken), l.terms);
            if (l.debt > Math.mulDiv(value, l.terms.borrowLtvBps, BPS)) revert Terms();
        }
        l.collateral = remaining;
        collateralLiability[l.collateralToken] -= amount;
        _pay(IERC20(l.collateralToken), l.borrower, amount);
        emit CollateralWithdrawn(id, amount);
    }

    function repay(bytes32 id, uint256 amount) external nonReentrant {
        Loan storage l = loans[id];
        if (amount == 0 || amount > l.debt) revert Terms();
        uint256 recoveredLoss = Math.min(amount, l.loss);
        l.loss -= recoveredLoss;
        l.debt -= amount;
        _repay(msg.sender, amount, amount - recoveredLoss);
        emit StockLoanRepaid(id, msg.sender, amount);
    }

    function loanHealth(bytes32 id)
        external
        view
        returns (uint256 collateralValue, uint256 maximumBorrow, uint256 liquidationDebt, bool liquidatable)
    {
        Loan storage l = loans[id];
        if (l.borrower == address(0)) revert Terms();
        collateralValue = _value(l.collateral, _price(l.terms, l.collateralToken), l.terms);
        maximumBorrow = Math.mulDiv(collateralValue, l.terms.borrowLtvBps, BPS);
        liquidationDebt = Math.mulDiv(collateralValue, l.terms.liquidationLtvBps, BPS);
        liquidatable = l.debt > liquidationDebt;
    }

    function liquidationQuote(bytes32 id, uint256 principal)
        public
        view
        returns (uint256 maximumRepay, uint256 seize)
    {
        Loan storage l = loans[id];
        if (l.borrower == address(0) || l.debt == 0 || l.collateral == 0) revert Terms();
        uint256 price = _price(l.terms, l.collateralToken);
        uint256 value = _value(l.collateral, price, l.terms);
        if (l.debt <= Math.mulDiv(value, l.terms.liquidationLtvBps, BPS)) revert Terms();
        uint256 cover = Math.mulDiv(value, BPS, BPS + l.terms.liquidationBonusBps);
        // A one-wei repayment lets the final dust collateral be cleared and bad debt accounted for.
        if (cover == 0) cover = 1;
        maximumRepay = Math.min(l.debt, cover);
        if (principal == 0 || principal > maximumRepay) revert Terms();
        if (cover < l.debt && principal == maximumRepay) {
            seize = l.collateral;
        } else {
            uint256 withBonus =
                Math.mulDiv(principal, BPS + l.terms.liquidationBonusBps, BPS, Math.Rounding.Ceil);
            uint256 usdtPrice = _feedPrice(address(usdtUsdFeed), usdtMaxAge);
            seize = Math.mulDiv(
                withBonus,
                usdtPrice * (10 ** (uint256(l.terms.tokenDecimals) + l.terms.feedDecimals)),
                price * (10 ** (18 + uint256(usdtFeedDecimals))),
                Math.Rounding.Ceil
            );
            if (seize > l.collateral) revert Terms();
        }
    }

    /// A liquidator repays actual USDT and receives at most the documented collateral bonus.
    function liquidate(bytes32 id, uint256 principal, uint256 minimumCollateral) external nonReentrant {
        Loan storage l = loans[id];
        (, uint256 seize) = liquidationQuote(id, principal);
        if (seize < minimumCollateral) revert Terms();
        l.debt -= principal;
        l.collateral -= seize;
        collateralLiability[l.collateralToken] -= seize;
        _repay(msg.sender, principal, principal);
        _pay(IERC20(l.collateralToken), msg.sender, seize);
        emit StockLoanLiquidated(id, msg.sender, principal, seize);
    }

    /// Public recognition of a fully liquidated shortfall. Loan debt persists and may still be repaid.
    function recognizeBadDebt(bytes32 id) external {
        Loan storage l = loans[id];
        if (l.borrower == address(0) || l.collateral != 0 || l.debt <= l.loss) revert Terms();
        uint256 loss = l.debt - l.loss;
        l.loss = l.debt;
        outstanding -= loss;
        emit LossRecognized(id, loss);
    }
}
