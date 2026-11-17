# Differences between the Rust port and the Python reference

The Rust binary reproduces the Python reference byte for byte on stdout, exit
codes, HTTP requests, and cache files, for every command over the fixture
corpus. This file lists every place where it deliberately does not, and why.
Anything that differs and is absent from this list is a defect.

The checks that enforce the list are the differential runner
(`parity/run.py`), the argument-surface check (`parity/argv_surface.py`), and
the oracle probe (`rust-tests/oracle_probe.rs`).

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

## Inputs the port does not accept

- **Non-ASCII digits.** Python reads any Unicode decimal digit as a number, in
  integer arguments, the daily request cap, numeric item IDs, relative time
  selectors (`٣d`), epoch strings, and vendor price text. Rust reads ASCII
  digits.
- **Integers beyond 64 bits** in integer arguments and the daily request cap.
  Python integers are unbounded.
- **Unpaired UTF-16 surrogates** in JSON strings. They cannot be Rust strings.
- **UTF-16 and UTF-32 JSON bodies.** Rust accepts UTF-8, with or without a
  byte-order mark.

## A stricter ForeverDB index

A ForeverDB index activates only when its record, vendor, and item counts agree
with its contents. Python checks the format number alone. Every index either
implementation publishes passes; the difference shows only for a file edited or
damaged after publication, which Rust treats as absent and answers from the
ninja cache.

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
