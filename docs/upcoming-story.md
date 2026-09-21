# Upcoming linked-work story — documentation only

**This is an illustrative contract discussion, not an accepted receipt, a saved
ledger, an executable event bundle or a CLI preview result.** Today's commands
persist and preview only the Phase 1 generation demo. The pure Phase 2 core has
broader typed evaluation, but the persistence bridge remains undefined. Do not
feed these amounts into the database or present them as booked history.

Imagine a customer asking a host to generate content, publish it, and attribute
an acquisition to the publication. Each arrow below represents a typed link to
earlier work, not one transaction covering the whole chain:

**Generation → publication → acquisition**

| Step | Who owes whom and why (illustrative accepted terms) | Separate economic responsibility |
|---|---|---|
| Generation | Customer owes host USD 1.00 for one successful generation. | Retail charge. |
| Publication | Publication links to generation; no new fee in this example. | Records related work; a link is not charging authority. |
| Acquisition | Customer owes host an additional USD 0.50 for an eligible, authorized acquisition attributed to that publication. | Separate retail premium; eligibility requires accepted terms, an authorized source and the agreed outcome window. |
| Customer discount | Host grants the customer USD 0.20 off the generation charge under separate accepted discount terms. | Retail discount only; this story does not assert an acquisition-discount implementation or change the unresolved Phase 2 discount rules. |
| Paid optimization tool | Host owes tool supplier USD 0.30 for authorized completed tool work. | Independent supplier obligation, requiring accepted supplier terms, assent, explicit roles, exposure and invocation authorization. The customer discount does not reduce this payable. |

These example prices express a story, not a new canonical contract or an
independent CLI calculator. They are intentionally not the frozen 80-atom demo
or the design's 120-atom onboarding fixture. No receipt IDs, canonical actions,
claim facts, snapshots or immutable history are fabricated here.

Funding responsibility must be explicit per obligation:

- **BYOK:** the customer contracted with the model provider. Provider usage/cost
  evidence may be observed separately; it does not create a host-to-provider
  supplier payable. The host's own retail price still follows its accepted terms.
- **Platform funded:** the host is the model supplier's payer under its accepted
  supplier agreement. An authorized supplier obligation belongs in the supplier
  book. A provider cost observation alone does not authorize that payable or a
  pass-through customer charge.
- **Paid tool:** select the real payer, bearer, beneficiary and recipient from
  accepted roles. If payer and bearer differ, the necessary delegation must be
  retained; possession of a tool invoice is insufficient authority.

The current `ledger preview` must continue rejecting unsupported linked events
without reserving identities, writing aliases, or producing a receipt.
Integration requires reviewed canonical encodings for the later facts and
snapshots, coordinator preparation/persistence, and stored explanation
projections. Only then can the CLI format those facade results and grow its
onboarding inputs. See [Phase 2 integration status](../PHASE-2-INTEGRATION-STATUS.md).
