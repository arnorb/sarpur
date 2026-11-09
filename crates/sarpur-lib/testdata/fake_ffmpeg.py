#!/usr/bin/env python3
"""A stand-in for ffmpeg in the Rust tests. The first -i URL picks the behaviour,
as `fake://<kind>` or a playlist named `fake-<kind>.m3u8` (`ruv::testing`):
ok writes the output and progress lines, fail exits 1 after writing part of
the output, slow writes progress until it reads `q`."""
import os
import select
import sys
import time

argv = sys.argv[1:]
url = argv[argv.index("-i") + 1]
out = argv[-1]
name = url.rsplit("/", 1)[-1]
if name.startswith("fake-") and name.endswith(".m3u8"):
    url = "fake://" + name[len("fake-"):-len(".m3u8")]


def progress(us, end=False):
    print(f"frame=1\nout_time_us={us}\nout_time_ms={us}\nprogress={'end' if end else 'continue'}", flush=True)


if url == "fake://ok":
    with open(out, "wb") as f:
        f.write(b"\0" * 1024)
    for us in (0, 75_000_000, 150_000_000, 299_000_000):
        progress(us)
    progress(300_000_000, end=True)
    sys.exit(0)
elif url == "fake://fail":
    with open(out, "wb") as f:
        f.write(b"\0" * 10)
    print("Server returned 404 Not Found", file=sys.stderr, flush=True)
    sys.exit(1)
elif url == "fake://slow":
    with open(out, "wb") as f:
        f.write(b"\0" * 10)
    us = 0
    while True:
        progress(us)
        us += 1_000_000
        ready, _, _ = select.select([sys.stdin], [], [], 0.1)
        if ready and sys.stdin.read(1) == "q":
            # ffmpeg finishes the file and exits 0 on q.
            sys.exit(0)
sys.exit(9)
