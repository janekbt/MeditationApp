#!/usr/bin/env python3
"""
Helper script invoked by Meson to run cargo build and copy the binary.

Usage: cargo-build.py <cargo> <manifest_path> <target_dir> <profile> <output>
  cargo         - path to cargo binary
  manifest_path - path to Cargo.toml
  target_dir    - cargo target directory
  profile       - "release" or "debug"
  output        - destination path for the built binary
"""
import os
import shutil
import subprocess
import sys

cargo = sys.argv[1]
manifest_path = sys.argv[2]
target_dir = sys.argv[3]
profile = sys.argv[4]
output = sys.argv[5]

cargo_flags = [
    "build",
    "--manifest-path", manifest_path,
    "--target-dir", target_dir,
]
if profile == "release":
    cargo_flags.append("--release")

env = os.environ.copy()
env.setdefault("CARGO_HOME", os.path.join(os.path.dirname(target_dir), "cargo-home"))

result = subprocess.run([cargo] + cargo_flags, env=env)
if result.returncode != 0:
    sys.exit(result.returncode)

src = os.path.join(target_dir, profile, "meditate")
shutil.copy2(src, output)
