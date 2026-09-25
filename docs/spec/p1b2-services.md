# Spec: P1 batch 2 — core services

Behavioral contracts for the 7 service files, derived from ground truth
only: the v0.10.72 oracle's observable behavior, the suite's behavioral
assertions, protocol facts (SNMP), and published test vectors
(PBKDF2/RFC, hashlib). Public API shapes stay identical per plan
decision 9; every body, doc, private helper, and test is written fresh.
Dead non-Linux branches are removed (Linux-only tree; behavior on Linux
is unchanged).

## actions/mod — privileged process control
Role: signal delivery and priority changes, argv-only, no shell.
Computation: binaries resolve through /usr/bin, /bin, /usr/sbin, /sbin
(first regular file wins; else the bare name). Kill runs
`kill -<signal> <pid>`; renice runs `renice -n <v> -p <pid>`. Nonzero
exit becomes an error naming the outcome.
Oracle: covered by error-path unit checks (bad pid fails cleanly).

## actions/run — alert command runner
Role: fire configured commands on triggers, safely.
Computation: a trigger is skipped when it repeats the recorded level
without the repeat flag, or inside the startup grace (2× refresh
seconds). Each command executes with failures logged, never raised;
the trigger level is recorded and true returned. Values render
`{{key}}`/`{{{key}}}` per argument AFTER quote-aware tokenizing, with
shell operators stripped from every value; section tags, unclosed tags,
and unclosed quotes refuse the command. Single-process mode runs one
argv; full mode chains `&&` segments, each with at most one `>` file
redirect and `|` pipelines spawned only after every stage tokenizes.
Stderr wins over stdout when non-empty.
Oracle: a refused template still records the trigger but creates no file.

## password/mod — credential file
Role: load, verify, and persist login credentials. File formats are
frozen interop (Python reads and writes the same file): `user:hex` is
plain sha256, `user:$sha256$salt$hash` is salted sha256 over
salt+password bytes, `user:salt$hex` (exactly one separator) is PBKDF2.
Computation: missing file loads empty; blank, `#`-comment, and
colon-less lines are skipped. Verification always runs a hash (dummy
for unknown users, so timing leaks nothing) and memoizes in a 32-entry
FIFO keyed by (user, sha256(password)). New entries use a 16-byte hex
salt and PBKDF2, clearing the cache. Saves create parent dirs, write
`user:hash` lines, and force 0600 even on pre-existing files. Salts
come from the kernel CSPRNG with a time-seeded fallback (uniqueness
token, not secret).
Oracle: format-parse matrix over all three shapes plus garbage lines.

## password/prompt — startup credential prompts
Role: establish session credentials from flags or the terminal.
Computation: server mode defines credentials, client mode only enters
them; other modes do nothing. No prompt flags means the glances/empty
defaults without touching stdin. A username prompt forces a password
prompt. Server side loops new-password plus confirmation until they
match, stores the hash, and warns (never fails) when the file won't
save; the session password stays unset. Client side reads one
password. Prompts go to stderr, input comes from stdin (EOF/error
reads empty). Echo toggles through `stty`, restored right after the
read; when that fails the read stays visible rather than blocking
startup. All prompt strings are frozen user-visible text.
Oracle: defaults path only (prompting needs a TTY; covered by review).

## pbkdf2 — password hashing primitive
Role: PBKDF2-HMAC-SHA256 with the file format's parameters (100 000
iterations, 128-byte key, hex-encoded; the salt is the raw string's
bytes, never hex-decoded). HMAC uses the standard 64-byte block with
long-key hashing. Iteration count and key length stay explicit
parameters so tests run cheap vectors. The RFC vector and the hashlib
vector are ground-truth fixtures and stay byte-exact.
Oracle: truncation and multi-block behavior on 1-iteration vectors.

## snmp/mod — agent model
Role: decoded values, OS detection, bulk reads, and the probe context.
Computation: values convert to f64 (ints direct, strings parsed) or to
str (strings only). OS detection lowercases sysDescr and takes the
first substring hit in table order (linux, darwin→mac, bsd, windows,
cisco, vmware esxi→esxi, netapp); empty or unmatched yields None.
Bulk reads return only answered pairs. Probing requires a non-empty
sysName answer, then maps sysDescr best-effort (missing stays None).
The sysDescr/sysName OIDs are protocol facts.
Oracle: OS matrix and version-refusal matrix (inline); loopback agent
round-trip (rewritten).

## snmp/client — UDP transport
Role: speak SNMP v1/v2c to an agent. Versions "1", "2c", "2" map;
"3" refuses (no USM in std); anything else errors naming the want
list. Default 3s timeouts on a fresh socket per request.
Computation: GET/GETNEXT/GETBULK share one builder (PDU tags a0/a1/a5,
bulk count in the GETBULK slot); GETBULK on v1 refuses. Responses must
carry the version, community, and RESPONSE tags, echo the request id,
and report status zero; varbinds decode to (OID, value) pairs. Walks
repeat bulk (25 rows) or single next steps, stopping outside the base
prefix, on exception markers, on stalled progress, or at the row
limit; back-to-back duplicates are skipped.
Oracle: loopback round-trip (inline, in snmp/mod tests).

## Test rewrites and oracle
`core_actions_run.rs` is rewritten with fresh names and structure
(echo/true only, temp-dir redirects). New oracle assertions append to
`core_oracle.rs` (template refusal, format matrix, KDF truncation).
