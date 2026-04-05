#!/usr/bin/env python3

from __future__ import annotations

import sys
from collections import deque
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter


def largest_component_bbox(mask: Image.Image) -> tuple[int, int, int, int] | None:
    width, height = mask.size
    pixels = mask.load()
    visited = bytearray(width * height)
    best_bbox = None
    best_area = 0

    for y in range(height):
        for x in range(width):
            index = y * width + x
            if visited[index] or pixels[x, y] == 0:
                continue

            queue = deque([(x, y)])
            visited[index] = 1
            min_x = max_x = x
            min_y = max_y = y
            area = 0

            while queue:
                cx, cy = queue.popleft()
                area += 1
                min_x = min(min_x, cx)
                max_x = max(max_x, cx)
                min_y = min(min_y, cy)
                max_y = max(max_y, cy)

                for nx, ny in ((cx - 1, cy), (cx + 1, cy), (cx, cy - 1), (cx, cy + 1)):
                    if nx < 0 or ny < 0 or nx >= width or ny >= height:
                        continue
                    neighbor_index = ny * width + nx
                    if visited[neighbor_index] or pixels[nx, ny] == 0:
                        continue
                    visited[neighbor_index] = 1
                    queue.append((nx, ny))

            if area > best_area:
                best_area = area
                best_bbox = (min_x, min_y, max_x + 1, max_y + 1)

    return best_bbox


def rounded_mask(
    size: tuple[int, int], bounds: tuple[int, int, int, int], radius: int
) -> Image.Image:
    width, height = size
    scale = 4
    large = Image.new("L", (width * scale, height * scale), 0)
    draw = ImageDraw.Draw(large)
    left, top, right, bottom = bounds
    draw.rounded_rectangle(
        (left * scale, top * scale, right * scale, bottom * scale),
        radius=radius * scale,
        fill=255,
    )
    return large.resize((width, height), Image.Resampling.LANCZOS)


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: render-notification-cutout.py <input> <output>", file=sys.stderr)
        return 1

    source = Path(sys.argv[1])
    target = Path(sys.argv[2])

    image = Image.open(source).convert("RGBA")
    width, height = image.size

    # The notification card sits on a distinct green background in the captured
    # screenshot. Detect the light card region, then cut only that card onto a
    # transparent canvas for a presentation-style asset.
    search_height = min(220, height)
    base_mask = Image.new("L", (width, search_height), 0)
    pixels = image.load()
    mask_pixels = base_mask.load()

    for y in range(search_height):
        for x in range(width):
            r, g, b, a = pixels[x, y]
            if a == 0:
                continue

            is_card = (
                r > 150 and g > 140 and b > 140 and abs(r - g) < 45 and abs(g - b) < 45
            )
            if is_card:
                mask_pixels[x, y] = 255

    bbox = largest_component_bbox(base_mask)
    if bbox is None:
        print("failed to locate notification card in screenshot", file=sys.stderr)
        return 1

    crop_pad = 10
    left = max(bbox[0] - crop_pad, 0)
    top = max(bbox[1] - crop_pad, 0)
    right = min(bbox[2] + crop_pad, width)
    bottom = min(bbox[3] + crop_pad, height)

    crop = image.crop((left, top, right, bottom))
    crop_width, crop_height = crop.size

    inset_left = bbox[0] - left
    inset_top = bbox[1] - top
    inset_right = inset_left + (bbox[2] - bbox[0])
    inset_bottom = inset_top + (bbox[3] - bbox[1])
    corner_radius = max(18, (bbox[3] - bbox[1]) // 5)

    mask = rounded_mask(
        (crop_width, crop_height),
        (inset_left, inset_top, inset_right, inset_bottom),
        corner_radius,
    )

    cutout = Image.new("RGBA", (crop_width, crop_height), (0, 0, 0, 0))
    cutout.paste(crop, (0, 0), mask)

    shadow_margin = 72
    shadow_offset = (0, 10)
    shadow_blur = 20

    canvas_width = crop_width + shadow_margin * 2
    canvas_height = crop_height + shadow_margin * 2
    canvas = Image.new("RGBA", (canvas_width, canvas_height), (0, 0, 0, 0))

    shadow_mask = rounded_mask(
        (canvas_width, canvas_height),
        (
            shadow_margin + inset_left + shadow_offset[0],
            shadow_margin + inset_top + shadow_offset[1],
            shadow_margin + inset_right + shadow_offset[0],
            shadow_margin + inset_bottom + shadow_offset[1],
        ),
        corner_radius,
    )
    shadow_mask = shadow_mask.filter(ImageFilter.GaussianBlur(radius=shadow_blur))

    shadow = Image.new("RGBA", (canvas_width, canvas_height), (0, 0, 0, 46))
    shadow.putalpha(shadow_mask)
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(cutout, (shadow_margin, shadow_margin))

    canvas = canvas.resize(
        (canvas.width * 2, canvas.height * 2),
        Image.Resampling.LANCZOS,
    )

    target.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(target)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
