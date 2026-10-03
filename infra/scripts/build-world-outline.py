#!/usr/bin/env python3
"""Regenerate client-gui/data/world-land.bin (Natural Earth 1:110m land, public domain).

Format, which nothing else can read without this:

    u16 polygon count
    per polygon: u16 point count, then (i16 lat, i16 lon) per point,
                 each scaled by 100.

110m is chosen for the globe's 106-150 px rendering; finer coastlines cost
bytes and change nothing on screen.
"""

import json
import os
import struct
import subprocess
import sys
import tempfile

URL = (
    "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/"
    "master/geojson/ne_110m_land.geojson"
)
SCALE = 100

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(REPO, "client-gui", "data", "world-land.bin")


from typing import Any


def rings(geometry: dict[str, Any]) -> list[list[list[float]]]:
    if geometry["type"] == "Polygon":
        return geometry["coordinates"]
    if geometry["type"] == "MultiPolygon":
        return [r for poly in geometry["coordinates"] for r in poly]
    return []


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "land.geojson")
        subprocess.run(["curl", "-sL", "-m", "180", "-o", src, URL], check=True)
        if os.path.getsize(src) == 0:
            sys.exit("empty download")
        data = json.load(open(src, encoding="utf-8"))

    polys: list[list[list[float]]] = []
    for feat in data["features"]:
        for ring in rings(feat["geometry"]):
            if len(ring) >= 4:
                polys.append(ring)

    buf = bytearray()
    buf += struct.pack("<H", len(polys))
    for ring in polys:
        buf += struct.pack("<H", len(ring))
        for lon, lat in ring:
            buf += struct.pack(
                "<hh", round(lat * SCALE), round(lon * SCALE)
            )

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "wb") as f:
        f.write(buf)

    print(f"polygons    {len(polys)}")
    print(f"points      {sum(len(r) for r in polys)}")
    print(f"wrote       {OUT} ({len(buf)} bytes)")


if __name__ == "__main__":
    main()
