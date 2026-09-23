# Pinned SQLite WAL high-water worksheet

This closes one source-level premise of the physical worksheet, not the full
retained-page, heap, filesystem-reservation or protected-completion gate.

For the linked SQLite3.51.3 build, let N be the enforced database max_page_count.
Fenced admission holds one actual writer and the shared physical gate, and requires
a successful wal_checkpoint(TRUNCATE) before BEGIN. The native caller does not use
sqlite3_db_cacheflush or expose external SQL writers. Reads cannot retain an older
WAL snapshot across this barrier. A valid engine-owned WAL index is assumed; corruption
is an operational/integrity failure, not an economic rejection.

In cached sqlite3.c70818–70820, walFrames identifies the first frame belonging to the
current transaction from the private versus live committed header. At70894–70912,
a subsequent spill of a page already in that transaction overwrites its existing
frame's page content rather than appending. The engine recomputes affected checksums
at commit. Thus merely calling pagerWalFrames repeatedly is not a counterexample to
a distinct-page bound. The last page of the final commit list may append one duplicate
to carry the commit marker. Savepoint undo at70579–70604 rewinds mxFrame and cleans the
hash; subsequent frames reuse the rewound area instead of increasing the high-water
for each retry. FULL-sync padding at70938–70965 uses a sector size capped at65536 bytes
at61613–61621.

The conservative allowance already used by backing_needed is therefore:

- Frames: N + 2 + ceil(65536/4120).
- WAL bytes: 32 + frames×4120 (4096-byte page plus24-byte frame header).
- WAL-index bytes: 32768×ceil((frames+34)/4096), accounting for4062 entries in the
  first32KiB region and4096 in later regions.

This argument permits the explicitly enabled spill threshold513. Disabling spilling
would instead retain dirty pages in memory and require a different funded heap bound;
it is not necessary solely to obtain this WAL-file bound.

The earlier audit concern inferred duplicate growth from pagerStress without following
walFrames and was withdrawn after inspection of the overwrite path. Independent
acceptance must still check these assumptions and exceptions on the exact integrated
source. Retained database growth, native workspace and actual reserved backing remain
separate open requirements.
