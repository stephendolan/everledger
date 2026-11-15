"""Inherited by refresh.sh's Python child; never installed outside parity runs."""
import os
import sys

if os.environ.get('EVERLEDGER_PARITY_ORACLE') == '1':
    try:
        from parity.oracle import install
        install()
    except BaseException as error:
        sys.stderr.write('parity clock installation failed: ' + str(error) + '\n')
        os._exit(125)
