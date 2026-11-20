"""Download one catalogue page through the public-source transport."""
from pathlib import Path
import sys
from everledger_core import Failure, request

if __name__ == '__main__':
    try:
        body, _ = request(sys.argv[1], {'User-Agent': sys.argv[3]}, public=True)
        Path(sys.argv[2]).write_bytes(body)
    except Failure as error:
        print(f'catalogue page fetch failed: {error}', file=sys.stderr)
        sys.exit(4)
