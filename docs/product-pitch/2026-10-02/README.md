# Bean Counter developer pitch — 2026-10-02

A reusable product-design snapshot: **Don’t rebuild billing for every product.**

Open `index.html` locally. Styles and interaction code are embedded; keep the adjacent SVG and linked documents together. No build dependencies, tracking, remote fonts, checkout, hosting configuration or credentials are included. The SVG embeds the original generated raster mascot without altering its pixels.

## Files
- `index.html`: developer-facing pitch and product-to-billing-engine diagram.
- `requirements.html`: proposed requirements and acceptance gates.
- `product.json`, `requirements.json`, `agent-brief.md`: machine-readable product scope and agent entry point.
- `design-review.html`: rationale, evidence and quality limitations.
- `bean-mark.svg`: original generated visor-bean mark; the page adds a finite CSS animation and reduced-motion support.

## Status and truth
This is a design/planning artifact, not a product release or implementation authorization. Current source scope was reviewed at `a139e03d77d7eba4d4f867baaa516754c598b0f4`. Source-built M4/M5 qualification is distinct from native-package qualification. Native distribution remains separately owned by issue #5 / PR #6.

Managed operations and arbitrary linked-work/multiple-milestone charging remain proposed/deferred. No price, launch date, conversion improvement, hosting SLA or universal plug-and-play integration is promised. The customer still owns instrumentation, mapping, identity retention, error handling and downstream adapters.

Static references, anchors, JSON correspondence and JavaScript syntax were checked. Selection logic was checked with a synthetic DOM. Browser visual, mobile, keyboard and screen-reader checks were not completed. No engine tests were run while preserving this pitch.

Only public-safe artifact content is included. Runtime/product code, dependencies, contracts, workflows and release settings are unchanged. Follow normal repository review and release gates before adoption.
