# Differences between the Rust port and the Python reference

The Rust binary reproduces the Python reference byte for byte on stdout, exit
codes, HTTP requests, and cache files, for every command over the fixture
corpus. This file lists every place where it deliberately does not, and why.
Anything that differs and is absent from this list is a defect.

The checks that enforce the list are the differential runner
(`parity/run.py`), the argument-surface check (`parity/argv_surface.py`), the
interrupt check (`parity/interrupts.py`), and the oracle probe
(`rust-tests/oracle_probe.rs`).

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

## Shape drift fails at the boundary

Where a payload does not have the shape the contract describes, Python limps
on or raises from deep inside a command. Rust refuses the payload where it is
decoded, with the exit code Python would have produced. This covers a
container where a scalar belongs, a number where a string belongs, and a
missing required field.

The ForeverDB attempt journal is one such payload. A sync needs its `spends`
to be a list in which each spend has a `finished_at` and a `requests` that is a
non-negative integer. Python's arithmetic takes a fraction, a boolean, or a
negative number as written, and a negative one enlarges the budget; it also
takes an object in place of the list. Rust refuses that journal when it syncs,
with exit code 1.

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

## A stricter ForeverDB index

A ForeverDB index activates only when its counts agree with its contents: the
record count with the vendor total, and the item count with the items. Python
checks the format number alone. Every index either implementation publishes
passes; the difference shows only for a file edited or damaged after
publication, which Rust treats as absent and answers from the ninja cache.

## Interrupts

Python takes Ctrl-C as an exception in the thread that is syncing. Rust takes
it on a second thread, which closes the journal and then ends the process as
SIGINT ends one. Both record what the attempt spent. They differ at the edges:

- **Between a change and its write.** An interrupt can land after the
  checkpoint changed in memory and before the write that follows. Python
  writes what it holds in memory; Rust writes the state it last wrote. The
  next attempt fetches that one page or detail again, inside its budget.
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
- An empty container in a cache file is falsy to Python, which then takes
  neither the path for a usable file nor the one for a damaged file. An empty
  object as `pending.json` is not discarded as a checkpoint of the wrong
  format, and every sync fails on its missing `sync_id` until the file is
  removed. An empty list as `attempt.json` reads as no journal to `status` and
  `vendors`. Rust discards that checkpoint and starts a new one, and refuses
  that journal.
- A negative amount that rounds to zero copper prints `0c`; Python prints `-0c`.
- Python finds a vendor page's `Last-Modified` header only in that exact letter
  case and otherwise reports no publication time. Rust finds it in any case.
- Lowercasing follows the Unicode version of the Rust toolchain. It differs
  from Python 3.13 on 75 letters added to Unicode since version 15.1.
