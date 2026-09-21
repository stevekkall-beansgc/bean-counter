# Fixture authority

All parties and events are synthetic. None represents consent from a real party.

`journals/first-slice` is the complete canonical acceptance oracle, preserved from the approved addendum. Do not add files inside it without an amendment: both independent builders assert the exact file set. The original compact submission is at `canonical/valid/first-slice-input.json`.

`journals/{first-party,third-party,capped,byok,reversal}/economics.json` freezes independent semantic postings, roles, totals and conditions from detailed design §7 and architecture §§9–10. These are test-oracle records, not production action schemas. Further full-journal variants require the encoding extension specified in the addendum.

`authority/cases.json` is an explicit scenario oracle. `authority/*-shape.json` tests only document shape; its referenced first-slice policy/assent does not authorize the illustrative supplier or delegated payer. Do not execute these as accepted setup. Full supplier authority must retain compatible accepted policy/assent/evidence when its implementation phase starts.

`failures/first-slice.json` enumerates each acceptance write and each loop-item before/after failpoint. These tests are requirements, not claimed database results. `canonical/invalid` contains intentionally invalid raw bytes and should fail before DTO parsing.
