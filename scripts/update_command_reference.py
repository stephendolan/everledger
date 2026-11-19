#!/usr/bin/env python3
"""Regenerate the command reference from a built Rust binary's help."""
import argparse
from pathlib import Path
import re
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path, help='path to the built everledger binary')
    args = parser.parse_args()
    binary = args.binary.resolve()
    reference = Path(__file__).resolve().parents[1] / 'docs/command-reference.md'
    help_text = subprocess.run([str(binary), '--help'], capture_output=True, check=True).stdout
    command_section = help_text.split(b'Commands:\n', 1)[1].split(b'\nOptions:', 1)[0]
    commands = [None, *re.findall(rb'^  ([a-z]+)\s', command_section, re.MULTILINE)]
    blocks = []
    for command in commands:
        argv = [str(binary)]
        if command is not None:
            argv.append(command.decode('ascii'))
        result = subprocess.run([*argv, '--help'], capture_output=True, check=True)
        if not result.stdout or result.stderr:
            raise RuntimeError(f'expected nonempty help and no stderr: {argv}')
        heading = b'' if command is None else b'## ' + command + b'\n\n'
        blocks.append(heading + b'```text\n' + result.stdout + b'```\n\n')
    reference.write_bytes(
        b'# Command reference\n\n'
        b'Generated from `--help`; regenerate with '
        b'`python3 scripts/update_command_reference.py target/release/everledger`.\n\n'
        + b''.join(blocks)
    )
    print(f'Updated docs/command-reference.md from {len(commands)} help invocations.')


if __name__ == '__main__':
    main()
