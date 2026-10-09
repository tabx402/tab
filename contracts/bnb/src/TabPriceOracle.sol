// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {Math} from "@openzeppelin/contracts/utils/math/Math.sol";

interface ITabPriceOracle {
    function token() external view returns (address);
    function quoteToken() external view returns (address);
    /// @return USDT units (18 decimals) per whole collateral token.
    function price() external view returns (uint256);
}

interface ITabAggregator {
    function decimals() external view returns (uint8);
    function latestRoundData() external view returns (uint80, int256, uint256, uint256, uint80);
}

/// @notice Immutable ratio of the exact collateral token/USD feed to USDT/USD.
/// @dev A tokenized equity needs a token-adjusted feed, not the underlying share price.
///      Stock splits, dividends and issuer multipliers must already be included in that feed.
contract TabPriceOracle is ITabPriceOracle {
    address public immutable token;
    address public immutable quoteToken;
    ITabAggregator public immutable collateralFeed;
    ITabAggregator public immutable quoteFeed;
    uint32 public immutable collateralMaxAge;
    uint32 public immutable quoteMaxAge;
    uint8 private immutable collateralDecimals;
    uint8 private immutable quoteDecimals;
    error InvalidFeed();

    constructor(address token_, address quote_, address collateral_, address quoteFeed_, uint32 maxAge_, uint32 quoteAge_) {
        if (token_.code.length == 0 || quote_.code.length == 0 || collateral_.code.length == 0
            || quoteFeed_.code.length == 0 || maxAge_ == 0 || maxAge_ > 2 days || quoteAge_ == 0 || quoteAge_ > 1 hours) revert InvalidFeed();
        token = token_;
        quoteToken = quote_;
        collateralFeed = ITabAggregator(collateral_);
        quoteFeed = ITabAggregator(quoteFeed_);
        collateralMaxAge = maxAge_;
        quoteMaxAge = quoteAge_;
        collateralDecimals = collateralFeed.decimals();
        quoteDecimals = quoteFeed.decimals();
        if (collateralDecimals > 18 || quoteDecimals > 18) revert InvalidFeed();
        price();
    }

    function _read(ITabAggregator feed, uint32 maxAge) private view returns (uint256) {
        (uint80 round, int256 answer,, uint256 updated, uint80 answered) = feed.latestRoundData();
        if (answer <= 0 || round == 0 || answered < round || updated == 0 || updated > block.timestamp
            || block.timestamp - updated > maxAge) revert InvalidFeed();
        return uint256(answer);
    }

    function price() public view returns (uint256) {
        uint256 asset = _read(collateralFeed, collateralMaxAge);
        uint256 quote = _read(quoteFeed, quoteMaxAge);
        uint256 result = Math.mulDiv(asset, 10 ** (18 + quoteDecimals - collateralDecimals), quote);
        if (result == 0) revert InvalidFeed();
        return result;
    }
}
