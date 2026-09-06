"""Executed by the Python actually installed by apt in the target app prefix."""
import json
import os
import subprocess
import sys

prefix = os.environ['PREFIX']
assert os.path.realpath(sys.executable).startswith(os.path.realpath(prefix) + os.sep), sys.executable

def output(args):
    return subprocess.check_output(args, text=True, timeout=30)

private = output([prefix + '/bin/printf', '%s', 'python-private-child'])
child = output([sys.executable, '-c', "print('python-child', end='')"])
shell = output(['/system/bin/sh', '-c', '"$PREFIX/bin/printf" %s python-shell-child'])
assert private == 'python-private-child', repr(private)
assert child == 'python-child', repr(child)
assert shell == 'python-shell-child', repr(shell)
print(json.dumps({'passed': True, 'executable': sys.executable, 'python_version': sys.version,
                  'private_child': private, 'python_child': child, 'shell_child': shell}, sort_keys=True))
