"""Draws OpenCheck's icon and writes it in the sizes the app and the
Microsoft Store package need:

- crates/opencheck-app/assets/icon.png (256 px, the window icon)
- packaging/msix/Assets/*.png (the package's logos)

Needs Pillow. Run from the repository root:  python scripts/app-icons.py
"""

from PIL import Image, ImageDraw

INK = (32, 37, 46, 255)
WHITE = (244, 244, 241, 255)
GREEN = (46, 158, 74, 255)
SIZE = 1024


def icon(size=SIZE, padding=0.0):
    """The mark: a scan frame with a check in it, on a graphite tile."""
    big = SIZE * 4
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = big * padding
    d.rounded_rectangle([pad, pad, big - pad, big - pad], radius=(big - 2 * pad) * 0.22, fill=INK)
    # The frame's corners.
    inner = pad + (big - 2 * pad) * 0.2
    outer = big - inner
    arm = (big - 2 * pad) * 0.17
    width = int((big - 2 * pad) * 0.055)
    for x, dx in ((inner, 1), (outer, -1)):
        for y, dy in ((inner, 1), (outer, -1)):
            d.line([(x, y + dy * arm), (x, y), (x + dx * arm, y)], fill=WHITE, width=width, joint="curve")
    # The check.
    s = big - 2 * pad
    points = [
        (pad + s * 0.33, pad + s * 0.52),
        (pad + s * 0.45, pad + s * 0.64),
        (pad + s * 0.68, pad + s * 0.38),
    ]
    d.line(points, fill=GREEN, width=int(s * 0.085), joint="curve")
    for p in (points[0], points[-1]):
        r = s * 0.0425
        d.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=GREEN)
    return img.resize((size, size), Image.LANCZOS)


def on_canvas(width, height, mark_size):
    """The mark centred on a transparent canvas, e.g. a wide tile."""
    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    mark = icon(mark_size)
    canvas.paste(mark, ((width - mark_size) // 2, (height - mark_size) // 2), mark)
    return canvas


icon(256).save("crates/opencheck-app/assets/icon.png")
assets = "packaging/msix/Assets"
icon(44).save(f"{assets}/Square44x44Logo.png")
icon(150, padding=0.12).save(f"{assets}/Square150x150Logo.png")
icon(50).save(f"{assets}/StoreLogo.png")
on_canvas(310, 150, 120).save(f"{assets}/Wide310x150Logo.png")
on_canvas(620, 300, 240).save(f"{assets}/SplashScreen.png")
# Taskbar and Start sizes without the plate behind them.
for size in (16, 24, 32, 48, 256):
    icon(size).save(f"{assets}/Square44x44Logo.targetsize-{size}_altform-unplated.png")
print("icons written")
