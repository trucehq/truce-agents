# ATDRP-0.1 conformance (ACP SEP #303)

`atdrp-provider.mjs` runs TruceHQ's production checker (`POST https://trucehq.com/api/atdrp/check`) through the conformance runner in agentic-commerce-protocol PR #316.

```bash
DISPUTE_PROVIDER_CMD="node atdrp-provider.mjs" node scripts/run-dispute-conformance.js \
  examples/unreleased/dispute_resolution/conformance_cases.json
```

Result on 2026-11-25: **33 of 33 cases agree at 284db2ece283** (case file sha256 284db2ece283415ddbcbdbd523e2a8452838e914c01beca8f44a3d0a597fd3cf), no not_evaluable cases.

TruceHQ is independent of the case author: we did not write or edit the cases. The checker was written from the RFC text; the cases were public while we built it.
