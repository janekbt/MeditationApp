#!/usr/bin/env python3
"""Replay fdroidserver's signing-key stripping on a build tree.

Before building, F-Droid runs `common.py remove_signing_keys()`, which
deletes the whole `signingConfigs { ... }` block and every
`signingConfig <value>` line from build.gradle — by regex, without
parsing Groovy. Anything that spans several lines therefore loses only
its first line and leaves a syntax error behind, which is how the 26.8.0
build failed on MR !43338 while building fine everywhere else.

Running this before a verification build reproduces their input exactly,
so that failure mode shows up here instead of in their CI.

Regexes and control flow are transcribed from fdroidserver master
(fdroidserver/common.py, gradle_* patterns + remove_signing_keys).

Usage: fdroid-strip-signing.py <build-dir>
"""

import os
import re
import sys

gradle_comment = re.compile(r'[ ]*//')
gradle_signing_configs = re.compile(r'^[\t ]*signingConfigs[ \t]*{[ \t]*$')
gradle_line_matches = [
    re.compile(r'^[\t ]*signingConfig\s*[= ]\s*[^ ]*$'),
    re.compile(r'.*android\.signingConfigs\.[^{]*$'),
    re.compile(r'.*release\.signingConfig *= *'),
]


def strip(path):
    with open(path) as fh:
        lines = fh.readlines()

    out, opened, i, changed = [], 0, 0, False
    while i < len(lines):
        line = lines[i]
        i += 1
        while line.endswith('\\\n'):
            line = line.rstrip('\\\n') + lines[i]
            i += 1

        # Comments are emitted before the block check, so a comment
        # inside signingConfigs survives while its code does not.
        if gradle_comment.match(line):
            out.append(line)
            continue
        if opened > 0:
            opened += line.count('{')
            opened -= line.count('}')
            changed = True
            continue
        if gradle_signing_configs.match(line):
            opened += 1
            changed = True
            continue
        if any(s.match(line) for s in gradle_line_matches):
            changed = True
            continue
        out.append(line)

    if changed:
        with open(path, 'w') as fh:
            fh.writelines(out)
    return changed


def main(root):
    touched = []
    for dirpath, _dirs, files in os.walk(root):
        for name in ('build.gradle', 'build.gradle.kts'):
            if name in files:
                path = os.path.join(dirpath, name)
                if strip(path):
                    touched.append(os.path.relpath(path, root))
                break
    for p in touched:
        print(f"stripped signing config from {p}")
    if not touched:
        # Must be fatal. repro-probe.sh runs under `set -e`, and a
        # silent no-op (wrong path, moved build.gradle) would let the
        # second build keep its signing config — then both APKs are
        # signed, the comparison ignores META-INF, and the probe reports
        # green while testing nothing. That is exactly the failure this
        # script exists to catch.
        print(f"no signing config found under {root}", file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    if len(sys.argv) != 2:
        sys.exit(f"usage: {os.path.basename(sys.argv[0])} <build-dir>")
    sys.exit(main(sys.argv[1]))
