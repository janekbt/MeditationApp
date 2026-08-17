#!/usr/bin/env python3
"""Compare two APK payloads entry by entry and report what differs.

Used by repro-probe.sh. Signature material (META-INF/*.SF, *.RSA, *.DSA,
MANIFEST.MF) is skipped: F-Droid's reproducible-build verification compares
payloads, not signature blocks.

Exit status is 0 when the payloads match, 1 when they don't — so the probe
can be wired into a check later without re-reading the output by eye.
"""

import hashlib
import re
import sys
import zipfile

SKIP = re.compile(r"^META-INF/(MANIFEST\.MF|[^/]+\.(SF|RSA|DSA|EC))$")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def entries(path):
    out = {}
    with zipfile.ZipFile(path) as z:
        for i in z.infolist():
            if SKIP.match(i.filename):
                continue
            out[i.filename] = (digest(z.read(i.filename)), i.file_size,
                               i.compress_type, i.date_time)
    return out


def strings(path, member, minlen=6):
    """Printable runs inside a zip member — enough to spot leaked paths."""
    with zipfile.ZipFile(path) as z:
        blob = z.read(member)
    return set(m.group().decode("latin-1") for m in
               re.finditer(rb"[ -~]{%d,}" % minlen, blob))


def main(a, b):
    ea, eb = entries(a), entries(b)
    only_a = sorted(set(ea) - set(eb))
    only_b = sorted(set(eb) - set(ea))
    common = sorted(set(ea) & set(eb))
    differing = [n for n in common if ea[n][0] != eb[n][0]]

    print(f"entries: {len(ea)} vs {len(eb)}   identical: "
          f"{len(common) - len(differing)}/{len(common)}")
    for n in only_a:
        print(f"  only in A: {n}")
    for n in only_b:
        print(f"  only in B: {n}")

    if not differing and not only_a and not only_b:
        print("\nRESULT: payloads are byte-identical across both paths.")
        return 0

    print(f"\ndiffering entries ({len(differing)}):")
    for n in differing:
        sa, sb = ea[n], eb[n]
        note = "" if sa[1] == sb[1] else f"  size {sa[1]} vs {sb[1]}"
        print(f"  {n}{note}")

    # For each differing entry, surface leaked build paths — the usual
    # culprit — by diffing printable strings and keeping the ones that look
    # like filesystem paths.
    for n in differing:
        try:
            sa, sb = strings(a, n), strings(b, n)
        except Exception:
            continue
        uniq = sorted(s for s in (sa - sb) if "/" in s)
        if not uniq:
            continue
        print(f"\n  strings present only in A's {n} (first 15):")
        for s in uniq[:15]:
            print(f"    {s[:160]}")

    print("\nRESULT: payloads differ.")
    return 1


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:3]))
