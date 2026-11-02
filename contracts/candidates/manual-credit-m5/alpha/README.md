# Manual credit request shapes — bounded alpha

This is a descriptive testing contract for `1.0.0-alpha.1`, not a finalized or independently certified billing contract. `requests.schema.json` describes the closed JSON shapes for issuing a credit and querying its status or retained record. It is a structural precheck, not an authority decision or a substitute for the ledger's validation. See [the tester guide](../../../../docs/manual-credit-alpha.md) for use and limitations.

Runtime additionally enforces the 262,144-byte request bound; customer and adjustment identifiers of at most 128 UTF-8 bytes with no control characters; exact canonical ordering of distinct Action refs; local owner and scope; and the complete original Work, Stage, Action, payer, recipient, Binding and producer joins. A matching JSON Schema does not prove any of those facts. The ledger's authoritative validation and the original accepted contract remain decisive.
