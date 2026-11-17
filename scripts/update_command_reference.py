#!/usr/bin/env python3
"""Regenerate the README command reference from a built Rust binary's help."""
import argparse
from pathlib import Path
import re
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path, help='path to the built everledger binary')
    args = parser.parse_args()
    binary = args.binary.resolve()
    readme = Path(__file__).resolve().parents[1] / 'README.md'
    prefix, reference = readme.read_bytes().split(b'```text\n', 1)
    reference, suffix = reference.split(b'## ForeverDB backend\n', 1)
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
    readme.write_bytes(prefix + b''.join(blocks) + b'## ForeverDB backend\n' + suffix)
    print(f'Updated README.md from {len(commands)} help invocations.')


if __name__ == '__main__':
    main()
