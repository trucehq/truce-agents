// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title TruceEscrow
/// @notice Minimal, non-custodial escrow for agent purchases with a neutral settlement step.
///         The buyer locks funds for a seller. Funds move only when:
///           1. the buyer releases them to the seller, or the seller refunds the buyer; or
///           2. the release deadline passes with no dispute, and the seller claims; or
///           3. both parties sign the same settlement split (EIP-712), typically the outcome of a
///              TruceHQ agreement, referenced by its agreement digest; or
///           4. a dispute is open and the dispute timeout passes with no signed settlement:
///              the funds are split evenly, so neither side gains by stalling.
///         TruceHQ (or any mediator) never holds keys, never signs and never receives funds here:
///         only the two parties' own signatures can move money. Unaudited template: audit before use.
interface IERC20 {
    function balanceOf(address who) external view returns (uint256);
}

contract TruceEscrow {
    enum Status { None, Funded, Disputed, Closed }

    struct Deal {
        address buyer;
        address seller;
        address token; // address(0) = native coin
        uint256 amount;
        uint64 releaseAt; // seller may claim after this if no dispute
        uint64 disputeTimeout; // seconds a dispute may stay open before the funds are split evenly
        uint64 disputedAt;
        Status status;
    }

    bytes32 public constant SETTLE_TYPEHASH =
        keccak256("Settle(uint256 dealId,uint256 toSeller,uint256 toBuyer,bytes32 agreementDigest)");
    bytes32 public immutable DOMAIN_SEPARATOR;

    uint256 public nextId = 1;
    mapping(uint256 => Deal) public deals;
    uint256 private locked = 1;

    event Opened(uint256 indexed id, address indexed buyer, address indexed seller, address token, uint256 amount, uint64 releaseAt);
    event Disputed(uint256 indexed id, address by);
    event Settled(uint256 indexed id, uint256 toSeller, uint256 toBuyer, bytes32 agreementDigest);
    event Released(uint256 indexed id, uint256 toSeller);
    event Refunded(uint256 indexed id, uint256 toBuyer);
    event Expired(uint256 indexed id, uint256 toSeller, uint256 toBuyer);

    error BadState();
    error NotParty();
    error TooEarly();
    error TooLate();
    error BadSplit();
    error BadSignature();
    error TransferFailed();

    modifier nonReentrant() {
        if (locked != 1) revert BadState();
        locked = 2;
        _;
        locked = 1;
    }

    constructor() {
        DOMAIN_SEPARATOR = keccak256(
            abi.encode(
                keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"),
                keccak256("TruceEscrow"),
                keccak256("1"),
                block.chainid,
                address(this)
            )
        );
    }

    /// @notice Buyer opens a deal and locks the funds.
    function open(address seller, address token, uint256 amount, uint64 releaseAt, uint64 disputeTimeout)
        external
        payable
        nonReentrant
        returns (uint256 id)
    {
        if (seller == address(0) || seller == msg.sender || amount == 0 || releaseAt <= block.timestamp) revert BadState();
        if (disputeTimeout < 1 days) revert BadState();
        if (token == address(0)) {
            if (msg.value != amount) revert BadState();
        } else {
            if (msg.value != 0) revert BadState();
            if (token.code.length == 0) revert BadState();
            // Count what actually arrived, so fee-on-transfer tokens work too.
            uint256 before = IERC20(token).balanceOf(address(this));
            _call(token, abi.encodeWithSelector(0x23b872dd, msg.sender, address(this), amount)); // transferFrom
            amount = IERC20(token).balanceOf(address(this)) - before;
            if (amount == 0) revert TransferFailed();
        }
        id = nextId++;
        deals[id] = Deal(msg.sender, seller, token, amount, releaseAt, disputeTimeout, 0, Status.Funded);
        emit Opened(id, msg.sender, seller, token, amount, releaseAt);
    }

    /// @notice Either party opens a dispute before the release deadline. Funds stay locked.
    function dispute(uint256 id) external {
        Deal storage d = deals[id];
        if (d.status != Status.Funded) revert BadState();
        if (msg.sender != d.buyer && msg.sender != d.seller) revert NotParty();
        if (block.timestamp >= d.releaseAt) revert TooLate();
        d.status = Status.Disputed;
        d.disputedAt = uint64(block.timestamp);
        emit Disputed(id, msg.sender);
    }

    /// @notice Buyer releases everything to the seller (happy path).
    function release(uint256 id) external nonReentrant {
        Deal storage d = deals[id];
        if (d.status != Status.Funded && d.status != Status.Disputed) revert BadState();
        if (msg.sender != d.buyer) revert NotParty();
        _close(id, d, d.amount, 0);
        emit Released(id, d.amount);
    }

    /// @notice Seller refunds everything to the buyer.
    function refund(uint256 id) external nonReentrant {
        Deal storage d = deals[id];
        if (d.status != Status.Funded && d.status != Status.Disputed) revert BadState();
        if (msg.sender != d.seller) revert NotParty();
        _close(id, d, 0, d.amount);
        emit Refunded(id, d.amount);
    }

    /// @notice Seller claims after the release deadline when nobody disputed.
    function claim(uint256 id) external nonReentrant {
        Deal storage d = deals[id];
        if (d.status != Status.Funded) revert BadState();
        if (block.timestamp < d.releaseAt) revert TooEarly();
        _close(id, d, d.amount, 0);
        emit Released(id, d.amount);
    }

    /// @notice Once a dispute has run past its timeout unsettled, anyone may split the funds evenly.
    ///         An even split gives neither side a reason to stall: disputing is never a free refund
    ///         for the buyer, and waiting it out is never a free payout for the seller.
    function expire(uint256 id) external nonReentrant {
        Deal storage d = deals[id];
        if (d.status != Status.Disputed) revert BadState();
        if (block.timestamp < uint256(d.disputedAt) + d.disputeTimeout) revert TooEarly();
        uint256 toSeller = d.amount / 2;
        uint256 toBuyer = d.amount - toSeller;
        _close(id, d, toSeller, toBuyer);
        emit Expired(id, toSeller, toBuyer);
    }

    /// @notice Pay out a split both parties signed. Anyone may submit the two signatures.
    /// @param agreementDigest e.g. the TruceHQ agreement_digest, so the payout points to the agreement.
    function settle(uint256 id, uint256 toSeller, uint256 toBuyer, bytes32 agreementDigest, bytes calldata sigBuyer, bytes calldata sigSeller)
        external
        nonReentrant
    {
        Deal storage d = deals[id];
        if (d.status != Status.Funded && d.status != Status.Disputed) revert BadState();
        if (toSeller + toBuyer != d.amount) revert BadSplit();
        bytes32 digest = keccak256(
            abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, keccak256(abi.encode(SETTLE_TYPEHASH, id, toSeller, toBuyer, agreementDigest)))
        );
        if (_recover(digest, sigBuyer) != d.buyer || _recover(digest, sigSeller) != d.seller) revert BadSignature();
        _close(id, d, toSeller, toBuyer);
        emit Settled(id, toSeller, toBuyer, agreementDigest);
    }

    function _close(uint256, Deal storage d, uint256 toSeller, uint256 toBuyer) private {
        d.status = Status.Closed;
        if (toSeller > 0) _pay(d.token, d.seller, toSeller);
        if (toBuyer > 0) _pay(d.token, d.buyer, toBuyer);
    }

    function _pay(address token, address to, uint256 amount) private {
        if (token == address(0)) {
            (bool ok, ) = to.call{value: amount}("");
            if (!ok) revert TransferFailed();
        } else {
            _call(token, abi.encodeWithSelector(0xa9059cbb, to, amount)); // transfer
        }
    }

    /// @dev Works with tokens that return true, return nothing (USDT-style) or revert on failure.
    function _call(address token, bytes memory data) private {
        (bool ok, bytes memory ret) = token.call(data);
        if (!ok || (ret.length != 0 && (ret.length != 32 || !abi.decode(ret, (bool))))) revert TransferFailed();
    }

    function _recover(bytes32 digest, bytes calldata sig) private pure returns (address) {
        if (sig.length != 65) revert BadSignature();
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := calldataload(sig.offset)
            s := calldataload(add(sig.offset, 32))
            v := byte(0, calldataload(add(sig.offset, 64)))
        }
        if (v < 27) v += 27;
        // Reject malleable high-s signatures.
        if (uint256(s) > 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0) revert BadSignature();
        address a = ecrecover(digest, v, r, s);
        if (a == address(0)) revert BadSignature();
        return a;
    }
}
