# Ledger Lab: adoption, cost and the first five minutes

Founder review · September 20, 2026 · Proposed product experience, not implemented software

Platform amendment: the [platform, cloud and hardware neutrality review](LEDGER-LAB-PLATFORM-NEUTRALITY-REVIEW.md) supersedes this report's installation/deployment assumptions where they differ. Native archives become the primary quickstart, the npm launcher remains optional, PostgreSQL examples become TCP-first, and named native test gates replace broad platform claims. Its section 13 lists the exact follow-up edits.

This reviews the [Rust foundation plan](LEDGER-LAB-RUST-FOUNDATION-PLAN.md) from the adopter's perspective. It preserves Rust, SQLite, PostgreSQL support and the atomic economic invariant, while recommending a smaller package and onboarding surface. It does not reopen the venture verdict or propose another general architecture.

**Direct answers:**

1. **The architecture is acceptable only after the simplifications below.** The internal correctness work is justified. Asking a new user to configure its full domain model would make it too complicated.
2. **Mandatory incremental founder cash cost can be $0.** No company-operated runtime infrastructure is required. Founder engineering and maintenance time are substantial; polished platform distribution and optional hosted services can cost money.
3. **The current plan does not yet solve cold start.** It can, through one prebuilt binary, one generated configuration, working templates and explicit event submission. “Under five minutes” is a release target to test, not a result established by this review.

All commands, package names, APIs and file layouts below are **proposed interfaces**. They have not been implemented or executed. `0.1.0` is an illustrative first public version; package names/availability must be confirmed before publishing. The copy/paste examples describe the required released experience, not something available to run today.

## 1. What a developer should actually have to learn

The initial mental model should be: **record work, apply its agreed price, and link related work when needed.** The result is an accepted receipt and a readable price breakdown. Customer identity and explicit terms remain real inputs; they do not require introducing every internal financial object on page one.

| Concept in the foundation plan | First-use treatment | When it becomes explicit |
|---|---|---|
| Event | Public: “something your application did.” Supply a stable operation/event ID. | Immediately. |
| Customer | Public: “who this work is for and whose accepted terms apply.” | Immediately for customer-priced work; a named synthetic customer in the demo. |
| Price/policy | Public as a short price rule or template. | Immediately, without a policy-language tutorial. |
| Link | Public as a typed reference to earlier work. | First chain example; optional for a single call. |
| Chain | Infer from a parent, or accept one application workflow ID. | Display it when there is related work. No chain registry setup. |
| Event source | Default to the authenticated local application principal. | Additional applications or external providers. Never infer identity from an untrusted body field. |
| Offer/price manifest | Absent from the first-party quickstart. | An external service proposes commercial terms. |
| Contract/binding | Present behind the accepted-price operation; show the selected terms/version. | Real customer consent, delegated payer or supplier agreement. A generated default cannot fabricate acceptance. |
| Policy snapshot/evaluator version | Generated and retained internally. | Available under explanation details or audit export. |
| Provider/cost originator | Default to the host for its own service; keep provider-cost data distinct. | BYOK, external model costs or paid tools. |
| Cost bearer/invoice payer | Same explicitly selected customer in the simple first-party preset; host for its own contracted supplier expense. | Expose overrides when they differ. Do not infer payer from beneficiary. |
| Beneficiary/revenue recipient | Default to the selected customer/host in that preset. | Agency/client arrangements, suppliers and shares. |
| Monetary actions | Show “base,” “premium,” “discount” and their amounts. | IDs and provenance are expandable details; no need to learn the action storage model. |
| Books/postings | Internal separation of customer obligations and supplier costs. | No chart of accounts or accounting vocabulary in onboarding. |
| Currency/rounding | Template chooses visible USD for the demo; real setup explicitly chooses its currency. Versioned rounding is a documented default. | Advanced terms only if the product later supports alternatives. |
| Intentions/outbox | Internal. User sees “export pending/sent/needs review.” | Adapter/operator diagnostics, never initial setup. |
| Adapter | Default fake destination. | The user asks to send results to another system. |
| Explanation | Public immediately: “why this amount?” | First accepted event. |
| Replay | Optional local debugger. | Testing changed prices or evidence; not a prerequisite for first value. |
| SQLite/PostgreSQL | SQLite selected automatically. | User chooses PostgreSQL for deployment/concurrency. |
| Tenant/environment | One local installation; sandbox separation generated. | Production setup, additional environments and scoped principals. |
| Idempotency/duplicates | Require a stable business-operation ID; SDK preserves requests on retry. | Explain duplicates as “already recorded,” not a new charge. |
| Canonicalization, hashing, locks, isolation, migrations | Internal implementation and operator commands. | Never application-integration prerequisites. |
| Missing dependencies | Plain status: “waiting for generation-19.” | Only when a referenced event has not arrived. Never call that accepted. |
| Authentication/keys | Generated local credentials, passed automatically to a local child application. | Remote deployment or an external event source. |

The six financial roles still exist in the engine. Their **defaults must be named mappings in a preset**, not guesses about real obligations. A simple host→customer relationship should take one customer field. A supplier arrangement must disclose who owes whom before it can create a payable.

The hard verdict is **acceptable only after simplification**. An SDK requiring users to construct offers, bindings, books, journals, snapshots and adapters before recording a $0.10 call would fail. The same underlying engine can be usable if those details are either generated from a clear accepted-price operation or revealed only when needed.

### Against realistic alternatives

| Alternative | Honest setup comparison | When Ledger Lab earns its extra moving part |
|---|---|---|
| Application code plus an existing billing integration | A developer who already uses a billing provider may add a simple usage charge with less work than installing another engine. Stripe currently recommends Metronome for most new usage integrations, with events sent to its ingest API. | Several linked operations, different customer/supplier terms, contextual adjustments or a need for local reproducible decisions. One flat counter is a weak reason to adopt Ledger Lab. |
| OpenMeter OSS | Its current repository quickstart uses Docker Compose and lists workers plus Kafka, ClickHouse, PostgreSQL, Redis and Svix. This is a richer stack than one local binary, not proof that every OpenMeter deployment has that exact footprint. | Ledger Lab can offer a much smaller local starting path for economic chains. It should not rebuild OpenMeter's broader metering platform. |
| x402/MPP-style pay-per calls | A paid-call protocol can be simpler if the complete requirement is “authorize payment and invoke this tool.” MPP already documents paid MCP calls. Its relevant payment method still brings payment authority/rail setup. | The price depends on related work, accepted contracts, different responsibility roles or shared proceeds. Ledger Lab must integrate the protocol instead of forcing its own payment system. |
| A small hand-written application table | Usually fastest for one price, one customer and no sharing. No extra daemon or SDK. | The developer is otherwise maintaining duplicate protection, immutable effects, linked price rules and explanations repeatedly. |

Sources: [Stripe usage guidance](https://docs.stripe.com/billing/subscriptions/usage-based/recording-usage), [OpenMeter OSS quickstart](https://github.com/openmeterio/openmeter/blob/main/quickstart/README.md), [x402](https://github.com/x402-foundation/x402), [MPP paid MCP tools](https://mpp.dev/guides/monetize-mcp-server).

Ledger Lab adds an engine process for JavaScript applications using HTTP. A friendly SDK does not remove that operational fact. Local `ledger dev` can manage it, while production users must run it themselves. Rust applications can embed the engine to avoid a separate process. This deployment tradeoff should be stated early in the integration guide.

## 2. The shortest path to an explained action

### One binary, one visible configuration, generated local state

Ship prebuilt binaries for a small tested platform set. A first user must not compile Rust, install PostgreSQL, run Docker, create a Ledger Lab account or obtain payment credentials. Offer a direct release archive and an optional npm launcher that selects a pinned platform binary and verifies its release digest. npm is an installation convenience; Node is not required by the Rust engine.

For Node users, the proposed bootstrap below needs an existing supported Node/npm installation and network access for the initial public package download. It needs no registry login. After installation/caching, the engine, schemas, examples and inspector run locally. For developers without Node, the release page must offer the corresponding binary archive with equally clear instructions. Do not make compiling through `cargo install` the advertised five-minute route.

Generated project:

```text
ledger-demo/
  ledger.yaml                 # the only file a beginner edits
  examples/generated.json
  examples/published.json
  .ledger/                    # generated, ignored by version control
    local.db
    local-credentials.json
    runtime.json              # only while dev is running
```

Accepted terms and snapshots are stored in the database, not a second hand-maintained manifest. Runtime discovery/credentials stay local and must not be committed or bundled into a browser application. `init` refuses to overwrite existing configuration or data. It creates explicit demo principals and accepted **synthetic** terms only when the user selects `--demo`.

The first configuration can be this small:

```yaml
schema: ledger/v1
mode: sandbox
currency: USD
prices:
  - on: content.generated
    charge: "1.00"
  - on: content.published
    from: {type: content.generated, relation: published_as}
    premium: "0.20"
```

This is a proposed preset-oriented surface, compiled into the bounded policy model. `published_as` means a generation event is the predecessor of the current publication event. It is not arbitrary YAML code. SQLite, the local principal, fake export destination and the demo customer are defaults created by the demo template. In a non-demo project, the same price file is a draft until explicitly associated with authorized accepted terms.

### Copy/paste target: first local atomic chain

**Proposed released quickstart; packages/commands are not available from this review.** The helper avoids global npm installation and administrator privileges:

```sh
ledger() { npx --yes --package=@ledgerlab/cli@0.1.0 ledger "$@"; }
ledger init ledger-demo --template content-chain --demo
cd ledger-demo
ledger accept examples/generated.json
ledger accept examples/published.json
ledger explain --chain demo-1
```

The generated event files are deliberately readable:

```json
{"id":"generation-1","type":"content.generated","customer":"demo-customer","chain":"demo-1","quantity":"1"}
```

```json
{"id":"publication-1","type":"content.published","customer":"demo-customer","chain":"demo-1","links":[{"relation":"published_as","from":"generation-1"}]}
```

Expected explanation:

```text
demo-1 · sandbox · demo terms v1
generation-1   Accepted   Base charge                 $1.00
publication-1  Accepted   Linked publication premium  $0.20
Total owed by demo-customer to demo-host              $1.20
Export destination: fake
Why: publication-1 publishes generation-1 under demo terms v1.
```

Each `accept` commits that event's complete decision atomically. **The two-event workflow is not one distributed transaction.** Retrying either file returns the original receipt and keeps $1.20. Submitting publication first returns a visible dependency wait; generation then makes it eligible for complete acceptance. The template must test that behavior, not merely display a pretty diagram.

`ledger dev` optionally starts the local API and bundled React inspector. The quickstart does not require it. The inspector should open directly to this chain, with the price breakdown and “why” visible. No empty dashboard, account creation, workspace naming sequence or billing-provider wizard.

### Small command surface

| Beginner command | Behavior |
|---|---|
| `ledger init [directory] --template NAME [--demo]` | Generate one starter, examples and safe local state. Demo mode explicitly uses synthetic accepted terms. |
| `ledger dev [-- COMMAND ...]` | Run local API/inspector; optionally launch a child app with endpoint/credentials supplied. Stop the child/session cleanly; no general workflow engine. |
| `ledger accept FILE [--preview]` | Accept one event, or calculate a clearly labeled noncommitting preview. Same identity on retry. |
| `ledger explain EVENT_ID` or `--chain ID` | Show result, applied price and supporting link/terms in plain language. |
| `ledger terms preview` / `ledger terms apply` | Validate and show the proposed economic change, then publish a version. In demo, apply can bind the synthetic parties; real counterparties require explicit authorization. |
| `ledger storage move --to postgres --url-env NAME` | Guided, verified offline cutover using a supplied empty database. |

Put backup, verify, export, advanced bindings and source administration under secondary help. They remain available without dominating the first page. If the local server owns SQLite, CLI commands connect to it through local discovery; they do not compete as another direct writer. Without a server, the CLI can use the embedded engine under the file ownership guard.

Editing YAML must never silently change accepted prices. `dev` can validate drafts and show a diff. Applying a version affects only newly authorized scope; existing event decisions and invocation terms remain fixed.

## 3. Adding it to an existing application

### TypeScript/JavaScript: explicit completion events first

Proposed commands in the existing application directory:

```sh
npm install @ledgerlab/sdk@0.1.0
npm install --save-dev @ledgerlab/cli@0.1.0
npx ledger init --template pay-per-call --demo
npx ledger dev -- node examples/ledger-demo.mjs
```

`init` writes `ledger.yaml`, `examples/ledger-demo.mjs`, an example completion event and ignored `.ledger/` state. It does not modify the application's business routes. The generated demo is a working integration test. For a TypeScript app, use its existing start command with `ledger dev --`, such as `npm run dev`; no second TypeScript runtime is prescribed.

The generated JavaScript example is approximately:

```js
import { Ledger } from "@ledgerlab/sdk";

const ledger = Ledger.fromEnv();
const receipt = await ledger.accept({
  id: "demo-transform-001",
  type: "tool.completed",
  customer: "demo-customer",
  quantity: "1"
});

console.log(receipt.status);
console.log(await ledger.explain(receipt.eventId));
```

The `pay-per-call` template defines a visible $0.10 successful-completion price and demo terms. `Ledger.fromEnv()` uses the local child process environment established by `ledger dev`; it does not scan arbitrary files or send credentials to a hosted service. In the real application, put the explicit call after the relevant success condition, use the durable application operation ID and the actual authorized customer binding. A separate preview method returns a different result type from a committed receipt.

The SDK supplies envelope mechanics, stable request retry, local authentication and readable errors. It cannot invent a durable operation ID, decide whether work succeeded or create payer consent. Optional event occurrence time remains explicit when the policy requires it; the SDK must not regenerate it on retry and produce a conflict. Host receipt time is separate metadata, not a fabricated producer fact.

Adding the call is first value, not a production reliability proof. A crash between external work and event submission needs the application's durable outbox/retry mechanism. A tool wrapper cannot make tool execution and a separate economic commit atomic. Document that gap at the production-integration step without burdening the synthetic quickstart.

### Rust: same starter, embedded engine available

In an existing Tokio Rust application, proposed commands are:

```sh
ledger init --template pay-per-call --demo
cargo add ledgerlab@0.1.0
```

If the application does not already use Tokio, its embedded async path also needs:

```sh
cargo add tokio --features macros,rt-multi-thread
```

Proposed minimal executable example, consuming the generated file:

```rust
use ledgerlab::Ledger;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ledger = Ledger::open("ledger.yaml").await?;
    let receipt = ledger
        .accept_json(include_str!("../examples/completed.json"))
        .await?;
    println!("{}", ledger.explain(&receipt.event_id).await?);
    Ok(())
}
```

The facade resolves the same starter configuration and local principal. Do not run another writer server against the same SQLite store while embedded mode owns it. Applications wanting a separate service can use HTTP instead. Compilation time depends on an existing toolchain/cache and is measured separately from the prebuilt-binary quickstart; a cold Rust build cannot honestly be promised to finish in five minutes everywhere.

### Switch to user-supplied PostgreSQL

Stop `ledger dev` with Ctrl-C, or stop the embedded writer. With a compatible local PostgreSQL server already installed/running and the developer authorized to create a database:

```sh
createdb ledgerlab
export LEDGER_POSTGRES_URL='postgresql:///ledgerlab'
ledger storage move --to postgres --url-env LEDGER_POSTGRES_URL
ledger explain --chain demo-1
ledger dev
```

For an existing remote database, the user supplies the approved URL through their normal secret mechanism instead of the local example. The command expects an empty target and suitable initialization privileges. It checks version/connectivity, takes a backup, freezes the source, copies exact history, verifies it, updates the backend configuration only after success and leaves the original fenced. It never rerates events. Unknown export outcomes must be resolved before dispatch resumes.

The resulting visible storage section is:

```yaml
storage:
  backend: postgres
  url_env: LEDGER_POSTGRES_URL
```

Credentials do not enter YAML or output logs. The SDK/event/price code stays the same. A fresh empty project can select PostgreSQL directly with the same storage section and initialization command. **Changing a URL alone does not migrate an existing ledger.** Database provisioning, permissions, TLS and production backups remain user responsibilities. The five-minute switch target below begins with a reachable empty database; provisioning is measured separately.

## 4. Cold start: begin with one observable piece of work

Do not ask users to design an ontology. Ask them to select one of five included templates and identify an application success point they already know: an image produced, an optimization completed, an item published or a confirmed acquisition. A stable existing operation ID plus one customer reference is enough to start a simple price rule.

| Template | Developer supplies | Defaults and next step |
|---|---|---|
| Pay per completed call | Completion event name, unit, price, operation ID and customer terms. | One successful unit; no charge on an explicitly failed completion. Show one base action. |
| BYOK / platform-funded | Funding mode from trusted host configuration, provider/cost origin and any known cost. | BYOK does not automatically create a host supplier payable. Platform-funded provider expense and retail price remain separate. Unknown cost stays unknown, not zero. |
| Chained tool fee | A completed external tool operation, predecessor ID, accepted supplier terms and who bears/pays the fee. | Host is the bearer only if explicitly accepted; named relation links the work. Show customer amount and supplier cost separately. |
| Outcome premium | Outcome event, predecessor, amount, authorized evidence source, claim identity and eligibility window. | No automatic causal inference; the approved source supplies the claim. A fixture source exercises the local example. |
| Customer-tier discount | Accepted tier/contract context, percentage or fixed amount, and named base. | Tier comes from the host binding, not a provider's self-selected field. Show discount as its own action. |

These are examples over a small shared policy vocabulary, not five separate products or an elaborate template marketplace. Start with the base/premium/discount templates and keep the supplier/share preset available as the second walkthrough. A user should discover its extra fields only when selecting “I pay another service.”

### Input priority

**First: explicit SDK/HTTP calls and individual JSON fixtures.** These establish economic intent and reliable identity most clearly. **Second: local NDJSON import in preview mode**, with a small field mapping from existing application logs/events. Report missing customer, unit, operation ID or trustworthy source; do not guess them. Importing a file processes candidates individually, not as an implicitly atomic batch.

**Later: opt-in tool wrappers and framework helpers.** A wrapper can capture start/success/failure and propagate a chain ID. It cannot decide consent, infer commercial success from arbitrary text, guarantee delivery across crashes or treat retries as new billable work. Avoid a general-purpose collector, observability agent or provider-discovery integration in v0.

Support a **local preview path**, not an autonomous “learning” engine. `ledger accept event.json --preview` and `ledger terms preview --events sample.ndjson` show proposed effects without accepted events, action IDs usable for settlement or runnable outbox entries. They must label missing prerequisites. No AI-generated price is automatically activated, and previewing data cannot promote it into accepted history.

### From synthetic to user-specific

The demo uses synthetic identities and explicitly accepted fixture terms. To customize, replace the event name, unit and amount in one YAML file, try one redacted application event and inspect the resulting explanation. Templates generate a proposed configuration and explain every economic choice. They do not certify the chosen policy as commercially correct.

Then create a **separate real environment** through a simple setup command and bind actual accepted terms. Do not offer “promote demo ledger to production.” Prices can be reused after review; synthetic history and demo authority cannot. The production guide should show a concrete binding command such as:

```sh
ledger bind customer-42 --terms retail-v1 --acceptance-ref order-42
```

For clarity, the proposed setup sequence before that binding is `ledger init ../ledger-real --template pay-per-call` without `--demo`, edit its `ledger.yaml`, and run `ledger terms preview` followed by `ledger terms apply --name retail-v1` from that directory. This publishes the reviewed version but does not create customer assent. The real application then uses that environment's endpoint/credentials and the actual customer ID. Store acceptance evidence through the binding operation; do not require users to hand-edit internal database records.

This is an authorized operator's record of existing acceptance, not a mechanism that obtains consent from another person. The reference must resolve to retained acceptance evidence. The tool displays parties, currency, basis and maximum relevant exposure before creating the binding. A host can agree to its own supplier costs; it cannot manufacture customer agreement by checking a box.

### Minimum configuration at each threshold

| Threshold | Required information |
|---|---|
| Noncommitting preview | Candidate event, draft price and enough typed context to calculate; missing authority is reported, not invented. |
| First committed demo action | Named synthetic parties/source, explicit demo selection, a versioned template price and stable event identity. `init --demo` establishes only that sandbox authority. |
| First real committed customer action | Authenticated scoped source, stable operation/claim ID, customer/payer mapping, currency/unit, versioned accepted price/binding and sufficient eligibility evidence. First-party defaults can collapse roles only when they actually coincide. |
| First third-party supplier obligation | Identified provider/recipient, accepted offer/version, authorized bearer/payer, eligible work/success semantics, units/limits, invocation authorization, and authorized source. Outcome terms additionally name the claim authority/window/basis. |

**There is a necessary complexity floor.** Ledger Lab can remove configuration machinery, but it cannot safely remove the facts “who agreed,” “what costs what,” “who owes whom,” and “what happened.” If users cannot answer those, a dry-run explanation is useful; silently booking money is not.

### Onboarding tests that expose cold-start traps

Test with competent developers who have not seen the domain model. Give them the README and a clean supported environment. They must complete the local chain with no live account, no founder assistance and no architecture-document reading. Also test offline use after the initial download.

Require these concrete outcomes: retry leaves the total unchanged; a missing predecessor explains what to submit next; an invalid decimal points to the exact field; a changed price produces a visible terms diff; no accepted real binding yields an actionable error/preview rather than a charge; a supplier event cannot award itself a new price; deleting a projection and rebuilding preserves history; and an unavailable PostgreSQL target leaves the SQLite source usable and unchanged before cutover starts.

Finally, ask each tester to replace the sample with one real-shaped event from their application. A fast synthetic demo followed by an unexplained jump to a 30-field configuration is still a cold-start failure.

## 5. Founder and user cash costs

**A credible public developer v0 can be built and distributed for $0 incremental mandatory cash using the existing machine, Codex subscription and local models.** This assumes existing hardware/internet and chooses a free public distribution path. It does not promise a paid-quality hosted service, a security audit, frictionless signed installers on every OS, or unlimited model/CI usage.

| Item | Mandatory incremental founder cash | Lowest-cost credible route / optional cost |
|---|---:|---|
| Development tools and Rust build | $0 | Existing machine, open-source toolchain/libraries. Time, electricity and disk are real consumption. |
| Codex/local models | $0 additional subscription required | Use existing access within its limits. New credits, hardware or another subscription are optional; wait/reduce scope instead of assuming more paid capacity. |
| Local end-user use | $0 service cost | Prebuilt binary and SQLite. User supplies machine/storage and maintains backups. |
| PostgreSQL development/testing | $0 license/hosting required | Native local PostgreSQL and disposable local test databases. No managed database or paid container desktop needed. |
| Public source hosting | $0 | Public GitHub repository under a free plan, or another user-supplied host. Maintainer account is required; adopter runtime account is not. |
| Public CI | $0 mandatory | Local tests plus standard GitHub-hosted runners for public-repository CI. Avoid paid larger runners; bound artifact retention and check storage/billing settings. Local CI remains the fallback. |
| Binary releases | $0 mandatory hosting | Public release assets and checksums. Building/testing extra architectures costs maintenance time; keep the matrix small. |
| npm launcher/TypeScript SDK | $0 public-package plan | Free public packages under an available maintainer scope. Publishing credentials/2FA or trusted publishing need setup. SDK generation itself needs local Node tools, not a paid service. |
| Documentation | $0 | Repository docs and GitHub Pages on its supplied domain. A custom domain or paid documentation service is optional. |
| Apple Developer ID/notarization | $0 if omitted from v0's promise | Normal Apple Developer Program membership is **US$99/year**, region-dependent pricing. If this polish is required, budget it or use an eligible waiver; do not claim it is inherently free. |
| Windows signing/store distribution | $0 if deferred | No Windows signed-installer promise in initial v0. Price the chosen certificate/channel only when it becomes a requirement. |
| Security/license maintenance | $0 mandatory vendor fee | Review dependencies, retain notices, patch and publish advisories using free tools. Skilled maintainer time remains mandatory; external audits/support are optional paid work. |
| Optional production deployment | $0 founder obligation | Users supply existing servers or pay their chosen provider. Compute, storage, backups, monitoring, TLS/domain operations and availability have real costs. |
| Actual model/tool/payment usage | $0 required for Ledger Lab demo | Synthetic examples and fake export need none. Real upstream APIs and payment rails can charge the workflow owner. |

Current primary evidence: [GitHub Free/public repositories](https://github.com/pricing), [Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions), [release assets](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases), [npm Free plan](https://www.npmjs.com/products), [public package publication](https://docs.npmjs.com/about-public-packages/), [GitHub Pages availability](https://docs.github.com/en/pages/getting-started-with-github-pages/what-is-github-pages), [Apple enrollment fee](https://developer.apple.com/programs/enroll/).

### The hidden costs are mainly maintenance and distribution friction

Rust, SQLx, bundled SQLite and PostgreSQL do not introduce a mandatory SaaS bill. They do introduce build-toolchain support, SQLite patch verification, two-store regression tests and recovery documentation. npm generation adds release coordination; Git hosting/registries add free accounts and credential maintenance. Apache-2.0 does not remove dependency-license obligations. None requires a commercial support contract to publish this bounded v0.

Public GitHub standard-runner availability does not justify unbounded storage, paid runner selection or silently billable private-repository workflows. Keep paid usage disabled or budget-capped, publish binaries as releases rather than relying on indefinite CI artifact retention, and make the full suite runnable locally. Free tiers are distribution conveniences, not runtime architecture dependencies.

The meaningful cash exception is **polished macOS distribution**. Apple describes Developer ID/notarization as part of its distribution trust path. Without it, support source builds and clearly documented platform behavior, but do not guarantee every unsigned downloaded binary will launch without user intervention. Do not tell users to disable security controls to meet an onboarding metric. A Linux prebuilt route can remain the initial five-minute reference path; measure macOS separately. [Apple Developer ID](https://developer.apple.com/developer-id/)

The cheapest credible path is public source, a narrow prebuilt release matrix, local PostgreSQL tests, public standard-runner CI, npm public SDK and repository/Pages docs. **Baseline new required subscriptions: $0/month. Baseline new required one-time service fees: $0.** Optional normal Apple membership adds **US$99/year** if selected. Production hosting is chosen and paid by the user; this review cannot assign one honest fixed cost to workloads of unknown size.

## 6. Simplify the implementation as well as the onboarding

The prior plan was too elaborate in packaging and too expansive in what it implied should be fully polished before the first public release. Keep its transaction and identity rules; reduce the number of public components and advanced capabilities.

| Foundation item | Revised v0 decision | Why |
|---|---|---|
| Both databases | **Keep real SQLite and PostgreSQL support.** | Explicit founder requirement and credible growth path. Hide backend choice initially; reduce supported deployment modes rather than defer PostgreSQL correctness. |
| Axum/HTTP | **Keep a thin local/server boundary.** | JS/TS users otherwise need native bindings or subprocess protocols. `dev` manages it; no standalone cloud API product. |
| OpenAPI | **Keep one small reviewed contract.** | Prevents API/SDK drift. Do not expose every administrative internal object as CRUD. |
| Generated SDK | **Generate types and keep a thin client.** | One `accept`/`explain` path is enough. No fluent financial-object framework, multiple generators or SDK engine copy. |
| Canonical hashing | **Keep the complete, tested profile internally.** | Identity, conflict detection and portable history depend on it. Defer public signature infrastructure, Merkle trees and external audit anchoring. |
| Books/double-entry-like structure | **Remove chart-of-accounts and accounting setup from v0.** | Typed obligations with debtor/creditor, currency, component and source explain the model. Keep cost/revenue/allocations separate; full accounting postings are not the product. |
| Ten workspace crates | **Consolidate to three production crates plus a development testkit.** | Most boundaries can be modules. Keep a separately enforced pure core and one acceptance implementation. |
| Migration tooling | **Keep one offline verified SQLite→PostgreSQL command and native backup/restore.** | Portability is a user promise. Defer live migration, merge, bidirectional convenience workflows and elaborate migration UI. |
| Concurrency work | **Keep races that protect supported economic invariants.** | Duplicate charges, overspent invocation limits or partial decisions cannot be deferred. Defer multi-region, global budgets and custom lock-free scheduling. |
| Policy DSL | **Small template-oriented rules only.** | Fixed/unit amounts, named-basis percentage, premium/discount, one share, exact reversal and bounded chain cap. Defer arbitrary graph patterns and broad operator catalogs. |
| Floors and complex cap allocation | **Defer general floors and cross-chain/running-period caps.** | They add closure/order semantics beyond the first use cases. Reject unsupported rules explicitly. A simple final chain-stage cap can remain. |
| Outbox | **Keep immutable intentions and one durable fake destination.** | Proves export identity/retry without live billing. Begin with one worker per installation; sophisticated multi-worker dispatch can follow. |
| Replay/conformance | **Keep exact replay and focused shared goldens/races.** | These protect the claim. Defer a replay management platform, exhaustive ecosystem certification and custom synchronization modeling. |
| React UI | **Small bundled read-only chain inspector, optional to use.** | Reuse existing components to make “why this amount?” immediate. No policy-builder application or analytics suite. |

### Smallest internal implementation

Use one repository and one coordinated release:

- **`ledgerlab-core`**: pure domain, validated price AST, canonicalization, exact evaluation and explanations. No database, HTTP, runtime or clock access.
- **`ledgerlab`**: the embedded facade, one acceptance coordinator, narrow store ports, separate SQLite/PostgreSQL modules and their migrations. SQLx/Tokio stay here. Defaults/configuration compile into explicit domain objects.
- **`ledgerlab-cli`**, exporting binary **`ledger`**: CLI, optional Axum routes, local application launcher and embedded inspector assets. It composes the engine; it does not calculate prices itself.
- **`ledgerlab-testkit`**, development-only: independent fixtures, fake destination, crash/race/backend parity and onboarding runners. The fake destination can be included in demo builds without becoming another service.

Crate names are proposed and subject to availability. Future vendor adapters can be ordinary modules/crates behind the established export interface. There is no public plugin ABI, dynamic loading or separate release process per module.

This is a change from the earlier ten-crate recommendation. The needed dependency boundary is the pure core versus infrastructure, plus one shared coordinator versus two dialects. Separate crates for every domain responsibility do not currently pay for themselves. Splitting the adapters later is straightforward if contributor or dependency needs justify it; their SQL and tests are separate from day one.

The smallest public SDK surface is `accept`, `explain`, `getChain` and `preview`, with a typed link helper. For third-party execution add an explicit scoped invocation-authorization operation. Advanced terms/source administration can remain CLI operations in v0. Users do not assemble journal transactions, construct action IDs or choose isolation levels.

**Revised public v0:** one binary; one starter configuration; the five templates; explicit SDK/HTTP events and local preview import; immutable atomic decisions; fixed/unit/percentage adjustments and one share pattern; exact full reversals; first-party and accepted external-tool bindings; both stores; fake export; CLI explanation and optional inspector; verified offline growth to PostgreSQL. No live billing adapter, general accounting model, marketplace, autonomous attribution, cloud collector or required hosted service.

## 7. Release gates for onboarding, not just correctness

Measure five metrics with fresh users. These are targets; none was measured in this planning review. Count documentation reading, commands, error recovery and initial binary download in the main quickstart result. Report OS/install route and whether tools were cached. Do not hide installation failures by timing only the final command.

| Metric | Proposed target | Exact end condition |
|---|---:|---|
| Time to first accepted event | ≤3 minutes from quickstart start on a supported prebuilt route | Durable accepted sandbox receipt, not queued or previewed. |
| Time to first explained action | ≤4 minutes cumulative | Tester can identify the amount, price rule and charged synthetic party without reading architecture docs. |
| Time to first linked chain | ≤5 minutes cumulative | Two accepted linked events, $1.20 explained total, and a retry leaving that total unchanged. |
| Time to first custom policy | ≤10 minutes from opening the customization task | Change one base/premium/discount rule, preview the difference, apply to newly authorized demo scope, and explain a new event while old actions stay unchanged. |
| SQLite→PostgreSQL switch | ≤5 minutes of operator work for a small fixture store, starting with a reachable empty compatible target | Verified identical history/IDs/totals, old source fenced and a successful new event on PostgreSQL. Report transfer time and provisioning separately. |

Initial gate: at least four of five fresh competent developers complete the main chain without assistance within the target; record all failures and prompts for help. A small pilot is usability evidence, not a reliable population-wide percentile. Also test one JS application and one Rust application integration, and at least one customized supplier workflow with explicit accepted terms.

Reject these false successes: a demo using a hidden hosted service; a build requiring a compiler but timing starting afterward; a preview labeled accepted; a customer automatically bound to unreviewed generated prices; a UI that cannot explain a zero-action event; and a database switch that starts an empty ledger while appearing to preserve history.

## 8. Final founder answer

**Is it too complicated?** The full model would be too complicated if exposed directly. **Ship it only with the smaller surface: events, agreed prices, customers and optional links; everything else defaults or appears when needed.** Keep rigorous internals and simplify the crate/module structure.

**Does it cost the founder money?** **Required incremental cash is $0, with $0 mandatory company-operated runtime infrastructure.** This uses existing development resources and free public distribution. Founder time and security maintenance are unavoidable. Normal Apple signing/notarization membership is an optional US$99/year path; production infrastructure belongs to users who choose to deploy it.

**Is setup simple enough?** **It can be, but the foundation plan alone was insufficient.** Make the proposed six-line local chain quickstart the release gate. Default to SQLite, include synthetic accepted terms and examples, show explanations immediately, and require no ontology, payment account or database setup. Actual customer/supplier obligations still require actual accepted terms.

The revised v0 is **one binary, one repository, one release and one generated configuration**, with both databases supported underneath. The five-minute target ends at a retried, explained two-event chain; custom prices and real supplier agreements are progressively disclosed next steps.

Users must never need to understand Cargo crate boundaries, SQLx metadata, SQLite WAL settings, PostgreSQL isolation/lock ordering, canonical JSON internals, hash construction, action-ID derivation, outbox leases, fencing tokens, schema migration numbering or projection checkpoint mechanics to integrate an application. Operator diagnostics may expose those details when useful. **Users do need to understand what happened, what price was agreed, who owes whom and why.**
