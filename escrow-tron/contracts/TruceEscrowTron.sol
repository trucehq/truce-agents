// SPDX-License-Identifier: MIT
pragma solidity >=0.8.20 <0.9.0;

/// @title TruceEscrowTron
/// @notice Tron (TVM) port of TruceEscrow: a minimal, non-custodial escrow for agent purchases with a
///         neutral settlement step. The buyer locks TRX or a TRC-20 token for a seller. Funds move only when:
///           1. the buyer releases them to the seller, or the seller refunds the buyer; or
///           2. the release deadline passes with no dispute, and the seller claims; or
///           3. both parties sign the same settlement split (TIP-712, the Tron form of EIP-712), typically
///              the outcome of a TruceHQ agreement, referenced by its agreement digest; or
///           4. a dispute is open and the dispute timeout passes with no signed settlement:
///              the funds are split evenly, so neither side gains by stalling.
///         TruceHQ (or any mediator) never holds keys, never signs and never receives funds here:
///         only the two parties' own signatures can move money.
///         BETA: unaudited. This deployment only accepts the coins listed at deployment and caps
///         every deal at a fixed maximum per coin. The list and caps are immutable: there is no
///         owner, no admin key and no way to change them after deployment.
///
///         Tron differences from the EVM version:
///         - address(0) means TRX, amounts in sun (1 TRX = 1,000,000 sun).
///         - TRC-20 transfers ignore the return value. USDT on Tron is reported to return false from
///           transfer() even when the transfer succeeded, so a return-value check could lock funds.
///           Instead every token movement is verified by the change in this contract's own balance.
///         - The TIP-712 chain id is whatever the TVM reports as block.chainid; read it from
///           eip712Domain() instead of hard-coding it in a wallet or backend.
///         - TRC-10 tokens are not supported. Do not attach TRC-10 tokens to any call.
interface ITRC20 {
    function balanceOf(address who) external view returns (uint256);
}

contract TruceEscrowTron {
    enum Status { None, Funded, Disputed, Closed }

    struct Deal {
        address buyer;
        address seller;
        address token; // address(0) = TRX
        uint256 amount; // in the coin's smallest unit (sun for TRX)
        uint64 releaseAt; // seller may claim after this if no dispute
        uint64 disputeTimeout; // seconds a dispute may stay open before the funds are split evenly
        uint64 disputedAt;
        Status status;
    }

    bytes32 public constant SETTLE_TYPEHASH =
        keccak256("Settle(uint256 dealId,uint256 toSeller,uint256 toBuyer,bytes32 agreementDigest)");
    bytes32 private constant DOMAIN_TYPEHASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
    bytes32 public immutable DOMAIN_SEPARATOR;
    /// @notice The chain id baked into DOMAIN_SEPARATOR (block.chainid at deployment).
    uint256 public immutable DOMAIN_CHAIN_ID;

    /// @notice Per-coin maximum deal size; 0 means the coin is not accepted. address(0) = TRX.
    mapping(address => uint256) public maxAmount;

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
    error NotAccepted();

    modifier nonReentrant() {
        if (locked != 1) revert BadState();
        locked = 2;
        _;
        locked = 1;
    }

    /// @param tokens coins accepted (address(0) = TRX); caps holds the maximum deal size for each.
    constructor(address[] memory tokens, uint256[] memory caps) {
        if (tokens.length != caps.length || tokens.length == 0) revert BadState();
        for (uint256 i = 0; i < tokens.length; i++) {
            if (caps[i] == 0) revert BadState();
            maxAmount[tokens[i]] = caps[i];
        }
        DOMAIN_CHAIN_ID = block.chainid;
        DOMAIN_SEPARATOR = keccak256(
            abi.encode(DOMAIN_TYPEHASH, keccak256("TruceEscrow"), keccak256("1"), block.chainid, address(this))
        );
    }

    /// @notice EIP-5267 style domain description, so wallets and backends can read the exact
    ///         name, version, chain id and contract the settle signature must use.
    function eip712Domain()
        external
        view
        returns (
            bytes1 fields,
            string memory name,
            string memory version,
            uint256 chainId,
            address verifyingContract,
            bytes32 salt,
            uint256[] memory extensions
        )
    {
        return (hex"0f", "TruceEscrow", "1", DOMAIN_CHAIN_ID, address(this), bytes32(0), new uint256[](0));
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
        uint256 cap = maxAmount[token];
        if (cap == 0 || amount > cap) revert NotAccepted();
        if (token == address(0)) {
            if (msg.value != amount) revert BadState();
        } else {
            if (msg.value != 0) revert BadState();
            if (token.code.length == 0) revert BadState();
            // Count what actually arrived: works for fee-on-transfer tokens and for tokens whose
            // transferFrom return value cannot be trusted.
            uint256 before = ITRC20(token).balanceOf(address(this));
            _call(token, abi.encodeWithSelector(0x23b872dd, msg.sender, address(this), amount)); // transferFrom
            uint256 afterBal = ITRC20(token).balanceOf(address(this));
            if (afterBal <= before) revert TransferFailed();
            amount = afterBal - before;
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
        _close(d, d.amount, 0);
        emit Released(id, d.amount);
    }

    /// @notice Seller refunds everything to the buyer.
    function refund(uint256 id) external nonReentrant {
        Deal storage d = deals[id];
        if (d.status != Status.Funded && d.status != Status.Disputed) revert BadState();
        if (msg.sender != d.seller) revert NotParty();
        _close(d, 0, d.amount);
        emit Refunded(id, d.amount);
    }

    /// @notice Seller (or anyone) claims for the seller after the release deadline when nobody disputed.
    function claim(uint256 id) external nonReentrant {
        Deal storage d = deals[id];
        if (d.status != Status.Funded) revert BadState();
        if (block.timestamp < d.releaseAt) revert TooEarly();
        _close(d, d.amount, 0);
        emit Released(id, d.amount);
    }

    /// @notice Once a dispute has run past its timeout unsettled, anyone may split the funds evenly.
    function expire(uint256 id) external nonReentrant {
        Deal storage d = deals[id];
        if (d.status != Status.Disputed) revert BadState();
        if (block.timestamp < uint256(d.disputedAt) + d.disputeTimeout) revert TooEarly();
        uint256 toSeller = d.amount / 2;
        uint256 toBuyer = d.amount - toSeller;
        _close(d, toSeller, toBuyer);
        emit Expired(id, toSeller, toBuyer);
    }

    /// @notice Pay out a split both parties signed (TIP-712 / EIP-712). Anyone may submit the two signatures.
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
        _close(d, toSeller, toBuyer);
        emit Settled(id, toSeller, toBuyer, agreementDigest);
    }

    function _close(Deal storage d, uint256 toSeller, uint256 toBuyer) private {
        d.status = Status.Closed;
        if (toSeller > 0) _pay(d.token, d.seller, toSeller);
        if (toBuyer > 0) _pay(d.token, d.buyer, toBuyer);
    }

    function _pay(address token, address to, uint256 amount) private {
        if (token == address(0)) {
            (bool ok, ) = to.call{value: amount}("");
            if (!ok) revert TransferFailed();
        } else {
            // Do not trust the return value (USDT on Tron is reported to return false on success).
            // Require that exactly `amount` left this contract. Less means the transfer did not
            // happen; more would eat into other deals' funds.
            uint256 before = ITRC20(token).balanceOf(address(this));
            _call(token, abi.encodeWithSelector(0xa9059cbb, to, amount)); // transfer
            uint256 afterBal = ITRC20(token).balanceOf(address(this));
            if (afterBal > before || before - afterBal != amount) revert TransferFailed();
        }
    }

    /// @dev Requires the call not to revert; ignores any return data. Balance checks by the caller decide
    ///      whether the transfer really happened.
    function _call(address token, bytes memory data) private {
        (bool ok, ) = token.call(data);
        if (!ok) revert TransferFailed();
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
        // On the TVM ecrecover returns the 20-byte address (no 0x41 prefix), the same form as msg.sender.
        address a = ecrecover(digest, v, r, s);
        if (a == address(0)) revert BadSignature();
        return a;
    }
}
