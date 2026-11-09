"""Fake commands for the reel tools' tests."""
import shlex
import sys
from pathlib import Path


def write_python_command(path, source):
    """Make `path` an executable command that runs Python `source`.

    The command is a /bin/sh launcher that runs this test's interpreter on
    `<name>.py` beside it. A `#!{sys.executable}` line would break when the
    interpreter's path contains a space: the kernel splits a shebang line at
    whitespace, while the shell takes the quoted path whole. Because the body
    file is named after the command, `Path(sys.argv[0]).stem` is the command's
    name.
    """
    path = Path(path)
    body = path.with_name(path.name + ".py")
    body.write_text(source)
    path.write_text(
        "#!/bin/sh\n"
        f'exec {shlex.quote(sys.executable)} {shlex.quote(str(body))} "$@"\n'
    )
    path.chmod(0o755)
    return path
