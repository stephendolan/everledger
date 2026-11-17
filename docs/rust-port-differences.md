# Differences between the Rust port and the Python reference

The Rust binary reproduces the Python reference byte for byte on stdout, exit
codes, HTTP requests, and cache files, for every command over the fixture
corpus. This file lists every place where it deliberately does not, and why.
Anything that differs and is absent from this list is a defect.

The checks that enforce the list are the differential runner
(`parity/run.py`), the argument-surface check (`parity/argv_surface.py`), the
interrupt check (`parity/interrupts.py`), the damaged-cache check
(`parity/damage.py`), and the oracle probe (`rust-tests/oracle_probe.rs`).

## Text on stderr

Exit codes and stdout still match in each of these cases.

- **Usage errors.** The argument parser's own wording differs from argparse's.
- **Internal failure text.** Where Python prints an exception (`error: 'latest'`,
  a traceback), Rust names the boundary that failed.
- **External tool text.** The Python reference shells out to curl for the
  catalogue pages and prints curl's message on failure.
- **Help.** `--help` output is the Rust parser's; the README command reference
  is generated from it.

Contract text is exact: failure messages the CLI defines, deprecation
warnings, resync notices, and the catalogue-age advisory.

## Transport failure detail

When a source cannot be reached at all, the message ends with the HTTP
library's wording in parentheses: Python prints
`source unreachable (<urlopen error [Errno 111] Connection refused>)` and Rust
prints `source unreachable (io: Connection refused (os error 111))`. The text
before the parentheses, the exit code, and everything else match. This is the
one difference that can reach stdout: `status` reports the message as the
unreachable source's `state`.

A response that ends inside its headers is a second case. Python's HTTP
library takes what arrived as a complete response with no body, so the command
fails on the empty body: exit code 1 from most sources, and from ForeverDB a
failed sync that reports an API shape mismatch. Rust's HTTP library takes it as
a connection that broke: the source is unreachable, with all that follows. A
command that needs the source exits 4, a sync records a failed attempt, and a
lookup or refresh that holds a cached copy serves it as stale. A sync through
the connector is the same on both sides, because Python makes its request.

Both implementations report every other malformed response as an unreachable
source: a status line that is not HTTP, a body or an error body shorter than
its declared length, a broken chunk.

## Shape drift fails at the boundary

Where a payload does not have the shape the contract describes, Python limps
on or raises from deep inside a command. Rust refuses the payload where it is
decoded, with the exit code Python would have produced. This covers a
container where a scalar belongs, a number where a string belongs, and a
missing required field.

The cache files are such payloads when something other than this program wrote
them: the ForeverDB journal, checkpoint, and index, and the ninja cache files.
Each field that every writer writes must be present and hold its type; a field
a writer may leave null may be null. A command that reads a file failing that
exits 1 with nothing on stdout, and sends no request Python would not have
sent. Python computes with what it finds, so the two differ wherever Python
gets by:

- **In the journal**, `spends` that are not a list of spends, each with a
  `finished_at` and a whole, non-negative `requests`; a `failed` that is not a
  boolean; a `status` that is not a number. A negative spend enlarges Python's
  budget, and an expired spend is dropped unread.
- **In the checkpoint**, a `next_offset` or `total_requests` that is not a
  number, and a seller row with a field missing or of another type. Python
  sends text for `next_offset` to the service as the offset.
- **In the index**, `items` that are not an object, a count or `fetched_at`
  missing, and a seller row with a field missing or of another type. Python
  answers the lookups that do not touch the field, and a refresh syncs and
  replaces the file. A list of items fails only when the next index is built
  from it, after the requests are spent.
- **In a ninja cache file**, a field missing or of another type. Python prints
  the rows as they are, and a refresh fetches the item again and replaces the
  file.
- **In the journal, checkpoint, or index**, a `sync_id` that is not text.
  Python compares whatever it finds.

Rust goes on refusing such a file until it is removed: no command writes over
a file it cannot read. Rust is never the more lenient of the two. Where Python
fails on a damaged file, Rust fails too.

## Inputs the port does not accept

- **Non-ASCII digits.** Python reads any Unicode decimal digit as a number, in
  integer arguments, the daily request cap, numeric item IDs, relative time
  selectors (`٣d`), epoch strings, vendor price text, and a quantity sent as
  text. Rust reads ASCII digits.
- **Integers beyond 64 bits** in integer arguments, the daily request cap, and
  quantities. Python integers are unbounded.
- **Unpaired UTF-16 surrogates** in JSON strings. They cannot be Rust strings.
- **UTF-16 and UTF-32 JSON bodies.** Rust accepts UTF-8, with or without a
  byte-order mark.

## Interrupts

Python takes Ctrl-C as an exception in the thread that is syncing. Rust takes
it on a second thread, which closes the journal and then ends the process as
SIGINT ends one. Both record what the attempt spent. They differ at the edges:

- **Between a change and its write.** An interrupt can land after the
  checkpoint changed in memory and before the write that follows. Python
  writes what it holds in memory; Rust writes the state it last wrote. The
  next attempt fetches that one page or detail again, inside its budget.
- **While a failure is being recorded.** An interrupt can land after a request
  has failed and before the journal and checkpoint that record the failure are
  both written. Python leaves each file as that moment finds it: the failure in
  the journal, or the journal still open. Rust records the attempt as
  interrupted and keeps its checkpoint, which is what both do when the
  interrupt lands while the request is in flight.
- **Before the signal thread exists.** An interrupt during the first journal
  write of an attempt ends the Rust process at once and can leave that write's
  temporary file behind. Python removes its temporary file as it unwinds.
- **SIGINT ignored at startup**, as for a background job of a non-interactive
  shell. Python stays deaf to the signal. Rust closes its journal and ends.

## CPython artifacts left behind

- `datetime.fromisoformat` accepts one stray character between the time and a
  UTC offset (`2026-11-14T10:30:15 Z`). Rust rejects it.
- CPython fails to convert a naive date within a day of its minimum or maximum
  (`0001-01-01`, `9999-12-31`). Rust applies one rule: an instant is valid from
  `0001-01-01T00:00:00Z` through `9999-12-31T23:59:59.999999Z`.
- CPython refuses to convert a decimal string of more than 4,300 digits, a guard
  on its unbounded integers that each interpreter can reconfigure. Rust reads
  the number when it fits 64 bits, so an integer argument or a daily request cap
  padded with thousands of leading zeros is accepted.
- A negative amount that rounds to zero copper prints `0c`; Python prints `-0c`.
- Python finds a vendor page's `Last-Modified` header only in that exact letter
  case and otherwise reports no publication time. Rust finds it in any case.
- Lowercasing follows the Unicode version of the Rust toolchain. It differs
  from Python 3.13 on 75 letters added to Unicode since version 15.1.
