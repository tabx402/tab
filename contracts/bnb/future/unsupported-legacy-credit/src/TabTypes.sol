// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

library TabTypes {
    struct Agent {
        address owner;
        string name;
        uint256 dailyCap;
        bytes32 policyHash;
        bool paused;
        uint32 version;
        uint256 dailySpent;
        uint64 spendDay;
    }

    struct Protocol {
        address authority;
        address usdt;
        address tabToken;
        address backing;
        address economics;
        uint16 feeBps;
        uint256 completedWorkUsdt;
        uint256 feesCollectedUsdt;
        uint256 buybackSpentUsdt;
        uint256 buybackTokensAcquired;
    }

    struct Job {
        bytes32 root;
        bytes32 parent;
        address buyer;
        address executor;
        bytes32 agent;
        uint256 budget;
        uint256 available;
        uint256 maxCall;
        uint64 deadline;
        bytes32 termsHash;
        bytes32 evidence;
        uint64 tools;
        uint256 rewardPaid;
        uint256 refunded;
        uint32 children;
        uint8 depth;
        uint8 state;
        bool paused;
        bool timelySubmitted;
        uint16 feeBps;
        uint256 feePaid;
        uint256 providerSpent;
        uint64 submittedAt;
        uint64 cancelledAt;
        address[] recipients;
    }

    struct JobTerms {
        bytes32 id;
        address executor;
        uint256 budget;
        uint256 maxCall;
        uint64 deadline;
        bytes32 termsHash;
        uint64 tools;
        address[] recipients;
    }

    struct Spending {
        uint256 available;
        uint256 totalFunded;
        uint256 totalSpent;
        uint256 totalWithdrawn;
    }

    struct Session {
        bytes32 agent;
        address owner;
        address signer;
        uint64 nonce;
        uint64 expiresAt;
        uint256 perCall;
        uint256 dailyCap;
        uint256 totalCap;
        uint256 totalSpent;
        uint256 dailySpent;
        uint64 spendDay;
        uint64 tools;
        bool revoked;
        address[] recipients;
    }

    struct SessionTerms {
        uint64 nonce;
        address signer;
        uint64 expiresAt;
        uint256 perCall;
        uint256 dailyCap;
        uint256 totalCap;
        uint64 tools;
        address[] recipients;
    }

    struct Payment {
        address recipient;
        uint256 amount;
        uint64 tool;
        uint64 paidAt;
        bytes32 receiptHash;
    }

    struct Launch {
        bytes32 agent;
        address creator;
        address token;
        bytes32 metadataHash;
        uint64 launchedAt;
        bool factoryToken;
    }

    struct Bond {
        bytes32 agent;
        address owner;
        address token;
        address beneficiary;
        uint256 amount;
        uint16 penaltyBps;
        uint8 state;
        uint256 penaltyPaid;
        uint256 returned;
    }

    struct Stake {
        uint256 amount;
        uint64 unlockAt;
    }

    struct Outcome {
        bytes32 agent;
        address pairedToken;
        uint64 closesAt;
        uint64 deadline;
        uint8 state;
        uint256 forUsdt;
        uint256 againstUsdt;
        uint32 forCount;
        uint32 againstCount;
        uint32 winningClaims;
        uint256 paidUsdt;
    }

    struct Position {
        uint256 amount;
        bool side;
        bool claimed;
        uint256 payout;
    }

    struct Backing {
        uint256 amount;
        uint8 decimals;
    }

    struct Credit {
        bytes32 agent;
        address lender;
        address borrower;
        address signer;
        uint256 funded;
        uint256 available;
        uint256 outstanding;
        uint256 totalSpent;
        uint256 totalRepaid;
        uint256 withdrawn;
        uint256 perCall;
        uint256 dailyCap;
        uint256 dailySpent;
        uint64 spendDay;
        uint64 expiresAt;
        uint64 tools;
        bool accepted;
        bool closed;
        address[] recipients;
    }

    struct CreditTerms {
        bytes32 id;
        bytes32 agent;
        address signer;
        uint256 amount;
        uint256 perCall;
        uint256 dailyCap;
        uint64 expiresAt;
        uint64 tools;
        address[] recipients;
    }
}
