#!/usr/bin/env python3
"""Chrome/band measurement for a real LightSpeed window capture (WF-081b).

Inputs:
  * a PNG captured from the live client area at 320 x 420 (or 320 x 760);
  * a UIA geometry JSON recorded with that same capture. Two shapes are
    accepted: `{"elements": [{name, type, x, y, w, h}, ...]}` (the
    `chrome-geometry-<lang>-420.json` dump this fixture writes), or the legacy
    `{"rects420": {"<name>": {x,y,w,h}}}` probe evidence.

Measurement method (documented so it is re-runnable):
  * rail band  - the top full-bleed plane. Found by scanning the left edge
    (x=4) for the first row that leaves the rail colour and stays changed for
    20 consecutive rows.
  * bottom bands - derived from the UIA rects of the action button and the
    footer buttons at the same client size: the action band is
    button_top - S2 .. button_bottom + S2 (S2=8), the footer band starts one
    hairline below the action band and ends at the client bottom. Chrome is
    rail + action band + hairline + footer (scroll region = the remainder).

Usage:
  python measure-chrome.py window-320x420-en-starved.png chrome-geometry-en-420.json
"""
import json
import sys

from PIL import Image

RAIL_MIN_RUN = 20
S2 = 8


def rail_bottom(path: str) -> int:
    image = Image.open(path).convert("RGB")
    width, height = image.size
    if width != 320:
        raise SystemExit(f"{path}: expected a 320 px-wide client capture, got {width}")
    top = image.getpixel((4, 4))
    for y in range(RAIL_MIN_RUN, height - RAIL_MIN_RUN):
        if image.getpixel((4, y)) != top:
            if all(image.getpixel((4, y + k)) != top for k in range(1, RAIL_MIN_RUN)):
                return y
    raise SystemExit(f"{path}: rail boundary not found")


def rects_by_name(geometry_path: str) -> dict:
    with open(geometry_path, "r", encoding="utf-8") as handle:
        data = json.load(handle)
    rects = {}
    if isinstance(data.get("elements"), list):
        for element in data["elements"]:
            name = element.get("name")
            if name and name not in rects:
                rects[name] = element
    for key in ("rects420", "rects"):
        for name, rect in (data.get(key) or {}).items():
            rects.setdefault(name, rect)
    return rects


def measure(capture: str, geometry: str | None) -> dict:
    image = Image.open(capture)
    height = image.size[1]
    border = rail_bottom(capture)
    result = {
        "capture": capture,
        "geometry": geometry,
        "client": [image.size[0], height],
        "rail_band_px": border,
    }
    if border >= height:
        return result
    if geometry:
        rects = rects_by_name(geometry)
        action = rects.get("STOP OPTIMIZING") or rects.get("OPTIMIZE MY ROUTE")
        footer = rects.get("Hide to tray") or rects.get("Settings")
        if action and footer:
            action_top = int(action["y"]) - S2
            action_bottom = int(action["y"]) + int(action["h"]) + S2
            footer_top = int(footer["y"]) - S2
            client_bottom = int(footer["y"]) + int(footer["h"]) + S2
            client_top = client_bottom - height
            result.update(
                {
                    "action_band_px": action_bottom - action_top,
                    "footer_band_px": client_bottom - footer_top,
                    "scroll_region_px": action_top - (client_top + border),
                    "chrome_px": border + (action_bottom - action_top) + (client_bottom - footer_top),
                    "action_label_height_px": int(action["h"]),
                    "footer_button_height_px": int(footer["h"]),
                    "uia_offset": f"action_top_abs={action_top} client_top_abs={client_top}",
                }
            )
    return result


def main() -> None:
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    capture = sys.argv[1]
    geometry = sys.argv[2] if len(sys.argv) > 2 else None
    print(json.dumps(measure(capture, geometry), indent=2))


if __name__ == "__main__":
    main()
