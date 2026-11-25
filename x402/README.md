# @trucehq/x402

A neutral dispute route for [x402](https://www.x402.org) payments. Free for AI agents.

x402 payments settle onchain in seconds and can't be reversed by a card network. That's great until a paid call goes wrong: a 500 after payment, an empty result, the wrong data. Today there is no standard place to take that. This package adds one.

- **Sellers** advertise Truce in their 402 response (`extensions.dispute`), so buyers know there is a fair route.
- **Buyers** keep a receipt of every paid call (transaction, network, payer, URL, status) and can open a Truce with that evidence.
- **Truce** hears both agents in private, proposes something fair on the merits, and nothing happens unless both sides (or their mandates) agree. Truce never holds funds and has no token.

```bash
npm install @trucehq/x402
```

## Seller: advertise the dispute route

```js
import { disputeExtension } from "@trucehq/x402";

// In your PaymentRequired (402) object:
const paymentRequired = {
  x402Version: 2,
  accepts: [/* ... */],
  extensions: {
    ...disputeExtension({
      contact: "ops@yourapi.ai",
      notifyUrl: "https://yourapi.ai/truce", // optional: we POST invite codes here
      refundPolicy: "Full refund if we return a 5xx after payment.",
    }),
  },
};
```

Answer automatically when a buyer opens a Truce:

```js
import { answerPaymentDispute } from "@trucehq/x402";

app.post("/truce", async (req, res) => {
  const out = await answerPaymentDispute(req.body, async (invite) => {
    const ok = await didWeServe(invite.transaction);
    return {
      statement: ok ? "We returned the data, log id 812." : "Our side failed after payment.",
      wants: ok ? "No refund" : "Refund in full",
      mandate: { max_refund_usd: 20 }, // private: never shown to the buyer
      authorizedBy: "API ops team",
    };
  });
  res.json(out);
});
```

## Buyer: keep receipts, open a Truce when needed

```js
import { withReceipts, readDisputeTerms, openPaymentDispute } from "@trucehq/x402";

const pay = withReceipts(paidFetch); // paidFetch: any fetch that pays x402, e.g. from @x402/fetch
const res = await pay("https://api.example.com/premium-data");

if (res.status >= 500) {
  const out = await openPaymentDispute({
    receipt: pay.receipts[0],
    amountUsd: 12,
    statement: "We paid and got a 500 with no data.",
    wants: "A full refund of $12",
    authorizedBy: "Sam Lee", // required: who asked your agent to act
    mandate: { min_refund_usd: 6 }, // private
    sellerTerms: await readDisputeTerms(first402Response), // optional
  });
  console.log(out.invite_code); // send to the seller's agent if they had no notify URL
}
```

`authorizedBy` is required: an agent should only open or answer a dispute for someone who asked it to.

## The `dispute` extension (proposal)

```json
"extensions": {
  "dispute": {
    "provider": "trucehq",
    "open": "https://trucehq.com/api/agent-disputes",
    "notify": "https://seller.example/truce",
    "contact": "ops@seller.example",
    "refundPolicy": "Full refund on 5xx after payment",
    "docs": "https://trucehq.com/agents"
  }
}
```

Provider-neutral by design: any dispute service can use the same shape. It works well with the x402 Offer and Receipt extension: a signed receipt is strong evidence in a Truce.

MIT · [trucehq.com/agents](https://trucehq.com/agents) · Truce is not a payment network and never moves money.
