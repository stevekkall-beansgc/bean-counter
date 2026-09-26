#!/usr/bin/env node
// Synthetic caller outbox for one ordinary billing accept request.
// Usage: node billing_outbox.mjs LEDGER BILLING_DIR CUSTOMER SOURCE EVENT_JSON OUTBOX_DIR
// Run one caller process per outbox on a private, durable local filesystem.
import { spawnSync } from 'node:child_process';
import { closeSync, existsSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, realpathSync, renameSync, writeSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { isDeepStrictEqual } from 'node:util';

function fail(message) { throw new Error(message); }
function syncDir(directory) {
  const fd = openSync(directory, 'r');
  try { fsyncSync(fd); } finally { closeSync(fd); }
}
function syncFile(path) {
  const fd = openSync(path, 'r+');
  try { fsyncSync(fd); } finally { closeSync(fd); }
}
function saveNew(path, bytes, directory) {
  const fd = openSync(path, 'wx', 0o600);
  try {
    let written = 0;
    while (written < bytes.length) {
      const count = writeSync(fd, bytes, written, bytes.length - written);
      if (count <= 0) fail('outbox write made no progress');
      written += count;
    }
    fsyncSync(fd);
  } finally { closeSync(fd); }
  syncDir(directory);
}
function same(a, b) { return isDeepStrictEqual(a, b); }
function runJson(args) {
  const result = spawnSync(args[0], args.slice(1), { encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 });
  if (result.error) fail(`ledger failed to start: ${result.error.message}`);
  let value;
  try { value = JSON.parse(result.stdout); } catch { fail(`ledger returned non-JSON output (exit ${result.status})`); }
  return [result.status, value];
}

if (process.argv.length !== 8) fail('usage: node billing_outbox.mjs LEDGER BILLING_DIR CUSTOMER SOURCE EVENT_JSON OUTBOX_DIR');
const [ledger, suppliedInstallation, customer, source, eventFile, outbox] = process.argv.slice(2);
const installation = realpathSync(suppliedInstallation);
const eventBytes = readFileSync(eventFile);
let event;
try { event = JSON.parse(eventBytes.toString('utf8')); } catch { fail('event must be strict JSON'); }
if (event?.schema !== 'ledger-event/1' || event.customer !== customer || typeof event.id !== 'string' || !event.id ||
    typeof event.operation_id !== 'string' || !event.operation_id) fail('event must contain stable id and operation_id');

if (!existsSync(outbox)) {
  mkdirSync(outbox, { mode: 0o700 });
}
const outboxStat = lstatSync(outbox);
if (!outboxStat.isDirectory() || outboxStat.isSymbolicLink())
  fail('outbox must be a private directory, not a symlink');
if (outboxStat.mode & 0o077) fail('outbox must have no group or other permission bits');
syncDir(dirname(outbox));
const marker = join(outbox, 'installation.path');
const installed = Buffer.from(installation + '\n' + customer + '\n' + source + '\n');
if (existsSync(marker)) {
  if (!readFileSync(marker).equals(installed)) fail('outbox belongs to another canonical installation or customer/source scope');
} else {
  saveNew(marker, installed, outbox);
}
syncFile(marker);
syncDir(outbox);
const request = join(outbox, 'request.json');
if (existsSync(request)) {
  const saved = readFileSync(request);
  if (!saved.equals(eventBytes)) fail('event bytes differ from the pending request; retain the original IDs and bytes');
  const prior = JSON.parse(saved.toString('utf8'));
  if (prior.id !== event.id || prior.operation_id !== event.operation_id) fail('saved request IDs differ');
} else {
  saveNew(request, eventBytes, outbox);
}
syncFile(request);
syncDir(outbox);

const [code, response] = runJson([ledger, 'billing', '--directory', installation, 'accept', '--customer', customer, '--source', source, request, '--json']);
if (code === 8) {
  console.error('Outcome unknown. Keep request.json pending and retry this identical file.');
  process.exit(8);
}
if (code !== 0 || !['accepted', 'duplicate'].includes(response?.status)) {
  console.error(`Accept not acknowledged (exit ${code}): ${JSON.stringify(response)}`);
  process.exit(code || 1);
}
const receipt = response.receipt;
const target = receipt?.body?.target;
if (receipt?.kind !== 'base-acceptance' || typeof receipt.id !== 'string' || !receipt.id ||
    typeof target !== 'string' || !target) fail('receipt ID or target is missing');
const [explainCode, history] = runJson([ledger, 'billing', '--directory', installation, 'explain', '--customer', customer, target, '--json']);
if (explainCode !== 0 || history?.schema !== 'ledger-billing-statement/2' || history.complete !== true)
  fail('complete target explanation unavailable; request remains unacknowledged');
const matches = history.entries?.filter(entry => entry.target === target &&
  same(entry.receipt, receipt) && entry.receipt.id === receipt.id &&
  entry.records?.filter(record => record.kind === 'event' &&
    record.body?.data?.operation_id === event.operation_id).length === 1) ?? [];
if (matches.length !== 1) fail('receipt does not match the retained original operation');

const receiptPath = join(outbox, 'receipt.json');
if (existsSync(receiptPath)) {
  if (!same(JSON.parse(readFileSync(receiptPath, 'utf8')), receipt))
    fail('saved receipt differs; investigate before acknowledging');
  syncFile(receiptPath);
  syncDir(outbox);
} else {
  const temporary = join(outbox, 'receipt.json.pending');
  const bytes = Buffer.from(JSON.stringify(receipt) + '\n');
  if (existsSync(temporary)) {
    if (!readFileSync(temporary).equals(bytes)) fail('incomplete or different pending receipt; investigate before acknowledging');
  } else {
    saveNew(temporary, bytes, outbox);
  }
  syncFile(temporary);
  syncDir(outbox);
  renameSync(temporary, receiptPath);
  syncFile(receiptPath);
  syncDir(outbox);
}
console.log(`Acknowledged ${response.status} with original receipt ${receipt.id}`);
