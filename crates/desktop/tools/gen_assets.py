#!/usr/bin/env python3
"""Generate Windows 7 style visual assets for the NT 6.1 desktop.

All small artwork is rendered with 4x supersampling and Lanczos
downsampling so circles, diagonals and rounded corners stay smooth.
The taskbar and the logon panel are true "baked glass": the blurred
wallpaper / logon background behind them is composited into the PNG.
"""

import os
import math
import random

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
ASSETS = os.path.join(ROOT, "crates", "desktop", "assets")
THEME = os.path.join(ASSETS, "theme")
FONTS = os.path.join(ASSETS, "fonts")
SRC = os.path.join(ROOT, "crates", "desktop", "src")
OPEN_SANS = os.path.join(ROOT, "resources", "fonts", "open-sans")

os.makedirs(THEME, exist_ok=True)
os.makedirs(FONTS, exist_ok=True)

SS = 4


class _ScaledDraw(object):
    def __init__(self, draw):
        self._draw = draw

    def _scale(self, value):
        if isinstance(value, (list, tuple)):
            return [self._scale(item) for item in value]
        if isinstance(value, (int, float)):
            return value * SS
        return value

    def _wrap(self, kwargs):
        out = dict(kwargs)
        if "width" in out:
            out["width"] = out["width"] * SS
        if "radius" in out:
            out["radius"] = out["radius"] * SS
        return out

    def ellipse(self, xy, **kw):
        self._draw.ellipse(self._scale(xy), **self._wrap(kw))

    def rectangle(self, xy, **kw):
        self._draw.rectangle(self._scale(xy), **self._wrap(kw))

    def rounded_rectangle(self, xy, **kw):
        self._draw.rounded_rectangle(self._scale(xy), **self._wrap(kw))

    def polygon(self, xy, **kw):
        self._draw.polygon(self._scale(xy), **self._wrap(kw))

    def line(self, xy, **kw):
        self._draw.line(self._scale(xy), **self._wrap(kw))

    def arc(self, xy, **kw):
        self._draw.arc(self._scale(xy), **self._wrap(kw))

    def point(self, xy, **kw):
        self._draw.point(self._scale(xy), **kw)


def ss_canvas(size, color=(0, 0, 0, 0)):
    width, height = size
    return Image.new("RGBA", (int(width * SS), int(height * SS)), color)


def ss_draw(image):
    return _ScaledDraw(ImageDraw.Draw(image))


def ss_finish(image, size):
    return image.resize(size, Image.LANCZOS)


def radial_gradient(size, center, radius, inner, outer):
    width, height = size
    image = Image.new("RGBA", size, outer)
    pixels = image.load()
    cx, cy = center
    for y in range(height):
        for x in range(width):
            distance = math.hypot(x - cx, y - cy) / radius
            t = min(max(distance, 0.0), 1.0)
            t = t ** 1.6
            r = int(inner[0] + (outer[0] - inner[0]) * t)
            g = int(inner[1] + (outer[1] - inner[1]) * t)
            b = int(inner[2] + (outer[2] - inner[2]) * t)
            pixels[x, y] = (r, g, b, 255)
    return image


def light_streaks(size, seed, count, color=(160, 200, 255)):
    random.seed(seed)
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    width, height = size
    for _ in range(count):
        x0 = random.randint(-200, width)
        y0 = random.randint(int(height * 0.2), height)
        x1 = random.randint(0, width + 200)
        y1 = random.randint(0, int(height * 0.8))
        bow = random.randint(-250, 250)
        thick = random.randint(2, 5)
        alpha = random.randint(30, 70)
        points = []
        for step in range(61):
            t = step / 60.0
            mx = (1 - t) ** 2 * x0 + 2 * (1 - t) * t * ((x0 + x1) / 2 + bow) + t ** 2 * x1
            my = (1 - t) ** 2 * y0 + 2 * (1 - t) * t * ((y0 + y1) / 2) + t ** 2 * y1
            points.append((mx, my))
        draw.line(points, fill=color + (alpha,), width=thick, joint="curve")
    layer = layer.filter(ImageFilter.GaussianBlur(3))
    halo = layer.filter(ImageFilter.GaussianBlur(9))
    result = Image.new("RGBA", size, (0, 0, 0, 0))
    result = Image.alpha_composite(result, halo)
    result = Image.alpha_composite(result, layer)
    return result


def glow_blob(size, center, radius, color, alpha):
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    cx, cy = center
    draw.ellipse([cx - radius, cy - radius, cx + radius, cy + radius], fill=color + (alpha,))
    return layer.filter(ImageFilter.GaussianBlur(radius // 2))


def vignette(size, strength=70):
    width, height = size
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    pixels = layer.load()
    for y in range(height):
        for x in range(width):
            edge = min(x, y, width - 1 - x, height - 1 - y)
            t = min(edge / 220.0, 1.0)
            pixels[x, y] = (4, 10, 24, int(strength * (1 - t)))
    return layer


def make_wallpaper():
    size = (1024, 768)
    base = radial_gradient(size, (512, 320), 900, (28, 74, 158), (6, 20, 48))
    base = Image.alpha_composite(base, glow_blob(size, (512, 300), 330, (90, 150, 235), 110))
    base = Image.alpha_composite(base, glow_blob(size, (512, 300), 150, (160, 205, 255), 90))
    base = Image.alpha_composite(base, light_streaks(size, seed=7, count=12))
    base = Image.alpha_composite(base, light_streaks(size, seed=23, count=5, color=(220, 240, 255)))
    base = Image.alpha_composite(base, vignette(size))
    base.save(os.path.join(THEME, "wallpaper.png"))


def make_logon_background():
    size = (1024, 768)
    base = radial_gradient(size, (512, 360), 950, (24, 66, 132), (4, 16, 38))
    base = Image.alpha_composite(base, glow_blob(size, (512, 340), 300, (70, 130, 215), 90))
    base = Image.alpha_composite(base, light_streaks(size, seed=91, count=7, color=(150, 195, 255)))
    base = Image.alpha_composite(base, vignette(size, 90))
    base.save(os.path.join(THEME, "logon_bg.png"))


def glass_panel(size, radius, top_color, bottom_color, border, scaled=False):
    """Rounded translucent gradient panel; with scaled=True the result
    stays in the supersampled space for further drawing."""
    width, height = size
    if scaled:
        canvas_size = (int(width * SS), int(height * SS))
        radius_px = radius * SS
        unit = SS
    else:
        canvas_size = size
        radius_px = radius
        unit = 1
    image = Image.new("RGBA", canvas_size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    for y in range(canvas_size[1]):
        t = y / max(canvas_size[1] - 1, 1)
        r = int(top_color[0] + (bottom_color[0] - top_color[0]) * t)
        g = int(top_color[1] + (bottom_color[1] - top_color[1]) * t)
        b = int(top_color[2] + (bottom_color[2] - top_color[2]) * t)
        a = int(top_color[3] + (bottom_color[3] - top_color[3]) * t)
        draw.line([(0, y), (canvas_size[0], y)], fill=(r, g, b, a))
    mask = Image.new("L", canvas_size, 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, canvas_size[0] - 1, canvas_size[1] - 1], radius=radius_px, fill=255)
    alpha = Image.composite(image.split()[3], Image.new("L", canvas_size, 0), mask)
    image.putalpha(alpha)
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle([0, 0, canvas_size[0] - 1, canvas_size[1] - 1],
                           radius=radius_px, outline=border, width=2 * unit)
    rim = Image.new("RGBA", canvas_size, (0, 0, 0, 0))
    rim_draw = ImageDraw.Draw(rim)
    rim_draw.rounded_rectangle([unit, unit, canvas_size[0] - 1 - unit, canvas_size[1] - 1 - unit],
                               radius=max(radius_px - unit, 1),
                               outline=(235, 248, 255, 110), width=unit)
    image = Image.alpha_composite(image, rim)
    highlight = Image.new("RGBA", canvas_size, (0, 0, 0, 0))
    hdraw = ImageDraw.Draw(highlight)
    hdraw.rounded_rectangle([2 * unit, 2 * unit, canvas_size[0] - 2 * unit, canvas_size[1] // 3],
                            radius=max(radius_px - 2 * unit, 1), fill=(255, 255, 255, 30))
    highlight = highlight.filter(ImageFilter.GaussianBlur(3 * unit))
    image = Image.alpha_composite(image, highlight)
    return image


def make_logon_panel():
    """Baked Aero glass panel: the logon background behind the panel is
    blurred into the bitmap, then tinted with the glass gradient."""
    size = (320, 300)
    radius = 14
    background = Image.open(os.path.join(THEME, "logon_bg.png")).convert("RGBA")
    px = (1024 - size[0]) // 2
    py = (768 - size[1]) // 2 - 60
    behind = background.crop((px, py, px + size[0], py + size[1]))
    glass = behind.filter(ImageFilter.GaussianBlur(9))
    tint = Image.new("RGBA", size, (18, 44, 86, 168))
    glass = Image.alpha_composite(glass, tint)
    shade = Image.new("RGBA", size, (0, 0, 0, 0))
    sdraw = ImageDraw.Draw(shade)
    for y in range(size[1]):
        t = y / (size[1] - 1)
        sdraw.line([(0, y), (size[0], y)], fill=(0, 0, 0, int(70 * t)))
    glass = Image.alpha_composite(glass, shade)
    mask = Image.new("L", size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius=radius, fill=255)
    alpha = Image.composite(glass.split()[3], Image.new("L", size, 0), mask)
    glass.putalpha(alpha)
    glow = ss_canvas(size)
    gdraw = ss_draw(glow)
    gdraw.rounded_rectangle([-1, 0, size[0], size[1] - 1], radius=radius + 2,
                            outline=(120, 190, 255, 140), width=3)
    glow = glow.filter(ImageFilter.GaussianBlur(10))
    glow = ss_finish(glow, size)
    glass = Image.alpha_composite(glass, glow)
    frame = Image.new("RGBA", size, (0, 0, 0, 0))
    fdraw = ImageDraw.Draw(frame)
    fdraw.rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius=radius,
                            outline=(170, 215, 250, 220), width=2)
    fdraw.rounded_rectangle([2, 2, size[0] - 3, size[1] - 3], radius=radius - 2,
                            outline=(235, 248, 255, 90), width=1)
    glass = Image.alpha_composite(glass, frame)
    frame_tile = glass_panel((112, 112), 10, (200, 225, 250, 235), (120, 170, 225, 235), (235, 245, 255, 255))
    glass.paste(frame_tile, ((size[0] - 112) // 2, 14), frame_tile)
    avatar = Image.open(os.path.join(THEME, "avatar.png")).convert("RGBA")
    glass.paste(avatar, ((size[0] - 96) // 2, 22), avatar)
    field = glass_panel((210, 30), 6, (10, 20, 36, 230), (18, 38, 66, 230), (60, 100, 150, 255))
    glass.paste(field, (28, 208), field)
    button = glass_panel((44, 30), 6, (60, 140, 220, 255), (20, 80, 160, 255), (150, 210, 255, 255))
    glass.paste(button, (252, 208), button)
    bd = ImageDraw.Draw(glass)
    bd.polygon([(272, 214), (288, 223), (272, 232)], fill=(255, 255, 255, 255))
    glass.save(os.path.join(THEME, "logon_panel.png"))


def make_icons():
    size = (48, 48)

    def finish(icon, name):
        icon = ss_finish(icon, size)
        shadow = Image.new("RGBA", size, (0, 0, 0, 0))
        sdraw = ImageDraw.Draw(shadow)
        sdraw.ellipse([6, 38, 42, 47], fill=(0, 8, 20, 110))
        shadow = shadow.filter(ImageFilter.GaussianBlur(3))
        icon = Image.alpha_composite(shadow, icon)
        icon.save(os.path.join(ASSETS, "imageres", name))

    # ---- Computer ----
    image = ss_canvas(size)
    draw = ss_draw(image)
    draw.rounded_rectangle([4, 6, 44, 34], radius=3, fill=(24, 40, 66, 255))
    draw.rounded_rectangle([6, 8, 42, 32], radius=2, fill=(8, 16, 30, 255))
    screen = radial_gradient((36 * SS, 24 * SS), (18 * SS, 8 * SS), 46 * SS, (95, 175, 245), (12, 60, 130))
    smask = ss_canvas((36, 24), (0, 0, 0, 0))
    sdraw = ss_draw(smask)
    sdraw.rectangle([0, 0, 35, 23], fill=(255, 255, 255, 255))
    smask = smask.split()[3]
    image.paste(screen, (6 * SS, 8 * SS), smask)
    shine = ss_canvas(size)
    sdr = ss_draw(shine)
    sdr.polygon([(8, 30), (20, 8), (28, 8), (12, 30)], fill=(255, 255, 255, 46))
    shine = shine.filter(ImageFilter.GaussianBlur(1.5 * SS))
    image = Image.alpha_composite(image, shine)
    draw = ss_draw(image)
    draw.rectangle([20, 34, 28, 38], fill=(150, 165, 185, 255))
    draw.rounded_rectangle([12, 38, 36, 42], radius=2, fill=(175, 190, 210, 255))
    draw.rounded_rectangle([12, 38, 36, 39], radius=1, fill=(225, 235, 248, 255))
    finish(image, "computer.png")

    # ---- Network (globe) ----
    image = ss_canvas(size)
    globe = radial_gradient((40 * SS, 40 * SS), (16 * SS, 12 * SS), 44 * SS, (120, 195, 250), (10, 55, 125))
    gmask = ss_canvas((40, 40), (0, 0, 0, 0))
    gmdraw = ss_draw(gmask)
    gmdraw.ellipse([0, 0, 39, 39], fill=(255, 255, 255, 255))
    gmask = gmask.split()[3]
    image.paste(globe, (4 * SS, 4 * SS), gmask)
    draw = ss_draw(image)
    for offset in (6, 18, 32):
        draw.ellipse([4 + offset // 2, 4, 44 - offset // 2, 44], outline=(190, 225, 255, 150), width=1)
    draw.line([(24, 4), (24, 44)], fill=(190, 225, 255, 150), width=1)
    draw.line([(4, 24), (44, 24)], fill=(190, 225, 255, 150), width=1)
    hl = ss_canvas(size)
    hdraw = ss_draw(hl)
    hdraw.ellipse([8, 5, 30, 22], fill=(255, 255, 255, 80))
    hl = hl.filter(ImageFilter.GaussianBlur(4 * SS))
    image = Image.alpha_composite(image, hl)
    finish(image, "network.png")

    # ---- Recycle Bin ----
    image = ss_canvas(size)
    draw = ss_draw(image)
    draw.polygon([(12, 16), (36, 16), (33, 42), (15, 42)], fill=(120, 180, 235, 190))
    for x in (18, 24, 30):
        draw.line([(x, 17), (x + (x - 24) // 4, 41)], fill=(200, 230, 255, 120), width=1)
    draw.rounded_rectangle([9, 12, 39, 17], radius=2, fill=(90, 150, 215, 235))
    draw.rounded_rectangle([9, 12, 39, 14], radius=1, fill=(170, 215, 250, 235))
    draw.polygon([(24, 24), (28, 31), (20, 31)], outline=(235, 248, 255, 220), width=2)
    sheen = ss_canvas(size)
    sdr = ss_draw(sheen)
    sdr.polygon([(14, 16), (20, 16), (17, 41), (15, 41)], fill=(255, 255, 255, 60))
    sheen = sheen.filter(ImageFilter.GaussianBlur(1.2 * SS))
    image = Image.alpha_composite(image, sheen)
    finish(image, "recycle-bin.png")

    # ---- Control Panel ----
    image = ss_canvas(size)
    panel = glass_panel((40, 40), 5, (235, 240, 248, 245), (165, 180, 200, 245), (90, 110, 140, 255), scaled=True)
    image.paste(panel, (4 * SS, 4 * SS), panel)
    draw = ss_draw(image)
    for y in (16, 30):
        draw.rounded_rectangle([10, y - 2, 38, y + 2], radius=2, fill=(60, 80, 110, 255))
        draw.rounded_rectangle([10, y - 2, 26, y + 2], radius=2, fill=(60, 140, 220, 255))
        draw.ellipse([24, y - 5, 32, y + 5], fill=(235, 243, 252, 255), outline=(80, 110, 150, 255))
    finish(image, "control-panel.png")


def make_taskbar():
    """Baked glass: blur the wallpaper bottom, tint it, then draw the
    highlight, gloss and tray decorations with supersampling."""
    width, height = 1024, 40
    wallpaper = Image.open(os.path.join(THEME, "wallpaper.png")).convert("RGBA")
    strip = wallpaper.crop((0, 768 - height, 1024, 768)).filter(ImageFilter.GaussianBlur(7))
    tint = Image.new("RGBA", (width, height), (16, 28, 48, 196))
    image = Image.alpha_composite(strip, tint)
    # Vertical brightness ramp (darker at the bottom, like Win7).
    shade = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    sdraw = ImageDraw.Draw(shade)
    for y in range(height):
        t = y / (height - 1)
        sdraw.line([(0, y), (width, y)], fill=(0, 0, 10, int(60 * t)))
    image = Image.alpha_composite(image, shade)
    # Gloss + highlight rendered supersampled.
    decor = ss_canvas((width, height))
    ddraw = ss_draw(decor)
    ddraw.ellipse([-300, -70, 700, 26], fill=(255, 255, 255, 36))
    ddraw.line([(0, 0), (width, 0)], fill=(190, 230, 255, 230), width=2)
    ddraw.line([(0, 2), (width, 2)], fill=(110, 175, 235, 110), width=1)
    nx = width - 118
    ddraw.line([(nx - 8, 8), (nx - 8, 32)], fill=(255, 255, 255, 70), width=1)
    for index in range(4):
        bar_h = 4 + index * 4
        x = nx + index * 6
        ddraw.rectangle([x, 28 - bar_h, x + 3, 27], fill=(225, 240, 255, 235))
    sx = nx + 34
    ddraw.polygon([(sx, 18), (sx + 6, 12), (sx + 6, 26)], fill=(225, 240, 255, 235))
    ddraw.arc([sx + 7, 14, sx + 15, 24], start=-60, end=60, fill=(225, 240, 255, 200), width=2)
    fx = nx + 58
    ddraw.polygon([(fx, 12), (fx + 12, 15), (fx + 12, 25), (fx, 28)], fill=(235, 244, 255, 235))
    decor = ss_finish(decor, (width, height))
    image = Image.alpha_composite(image, decor)
    image.save(os.path.join(THEME, "taskbar.png"))


def make_orb():
    size = (36, 36)
    image = ss_canvas(size)
    draw = ss_draw(image)
    draw.ellipse([0, 0, 35, 35], fill=(14, 30, 52, 255))
    draw.ellipse([2, 2, 33, 33], fill=(30, 80, 150, 255))
    inner = radial_gradient((30 * SS, 30 * SS), (15 * SS, 12 * SS), 30 * SS, (120, 200, 255), (10, 60, 140))
    mask = ss_canvas((30, 30), (0, 0, 0, 0))
    mdraw = ss_draw(mask)
    mdraw.ellipse([0, 0, 29, 29], fill=(255, 255, 255, 255))
    mask = mask.split()[3]
    image.paste(inner, (3 * SS, 3 * SS), mask)
    draw = ss_draw(image)
    panes = [(255, 120, 40), (140, 200, 60), (60, 170, 240), (250, 210, 60)]
    coords = [(9, 9), (19, 9), (9, 19), (19, 19)]
    for (px, py), color in zip(coords, panes):
        draw.rounded_rectangle([px, py, px + 8, py + 8], radius=1, fill=color + (255,))
    gloss = ss_canvas(size)
    gdraw = ss_draw(gloss)
    gdraw.ellipse([5, 2, 31, 18], fill=(255, 255, 255, 70))
    gloss = gloss.filter(ImageFilter.GaussianBlur(4 * SS))
    image = Image.alpha_composite(image, gloss)
    # Outer glow halo.
    halo = ss_canvas(size)
    hdraw = ss_draw(halo)
    hdraw.ellipse([1, 1, 35, 35], outline=(130, 200, 255, 120), width=2)
    halo = halo.filter(ImageFilter.GaussianBlur(3 * SS))
    image = Image.alpha_composite(image, halo)
    image = ss_finish(image, size)
    image.save(os.path.join(THEME, "orb.png"))


def make_brand_flag():
    size = (32, 28)
    image = ss_canvas(size)
    draw = ss_draw(image)
    panes = [
        ((255, 140, 60), (2, 1), (15, 12)),
        ((150, 205, 70), (17, 1), (30, 12)),
        ((70, 175, 245), (2, 14), (15, 25)),
        ((250, 215, 70), (17, 14), (30, 25)),
    ]
    for color, (x0, y0), (x1, y1) in panes:
        draw.rounded_rectangle([x0, y0, x1, y1], radius=1, fill=color + (255,))
    image = ss_finish(image, size)
    image.save(os.path.join(THEME, "brand_flag.png"))


def make_buttons():
    ease = glass_panel((24, 24), 12, (36, 66, 104, 235), (16, 36, 66, 235), (150, 200, 250, 220), scaled=True)
    draw = ss_draw(ease)
    draw.ellipse([9, 5, 15, 11], fill=(235, 245, 255, 255))
    draw.arc([7, 12, 17, 22], start=20, end=160, fill=(235, 245, 255, 255), width=3)
    ease = ss_finish(ease, (24, 24))
    ease.save(os.path.join(THEME, "btn_ease.png"))

    shut = glass_panel((24, 24), 12, (36, 66, 104, 235), (16, 36, 66, 235), (150, 200, 250, 220), scaled=True)
    sdraw = ss_draw(shut)
    sdraw.arc([6, 7, 18, 19], start=300, end=240, fill=(255, 210, 160, 255), width=3)
    sdraw.line([(12, 4), (12, 12)], fill=(255, 210, 160, 255), width=3)
    shut = ss_finish(shut, (24, 24))
    shut.save(os.path.join(THEME, "btn_shutdown.png"))


def make_avatar():
    size = (96, 96)
    image = radial_gradient((96 * SS, 96 * SS), (48 * SS, 42 * SS), 110 * SS, (150, 195, 240), (40, 90, 160))
    draw = ss_draw(image)
    draw.ellipse([34, 22, 62, 50], fill=(235, 244, 252, 255))
    draw.ellipse([22, 52, 74, 110], fill=(235, 244, 252, 255))
    frame = ss_canvas(size)
    fdraw = ss_draw(frame)
    fdraw.rounded_rectangle([0, 0, 95, 95], radius=8, outline=(240, 248, 255, 255), width=3)
    fdraw.rounded_rectangle([3, 3, 92, 92], radius=6, outline=(90, 150, 215, 200), width=2)
    gloss = ss_canvas(size)
    gdraw = ss_draw(gloss)
    gdraw.rounded_rectangle([2, 2, 93, 40], radius=6, fill=(255, 255, 255, 60))
    gloss = gloss.filter(ImageFilter.GaussianBlur(5 * SS))
    image = ss_finish(Image.alpha_composite(image, gloss), size)
    frame = ss_finish(frame, size)
    image = Image.alpha_composite(image, frame)
    image.save(os.path.join(THEME, "avatar.png"))


FONT_SIZES = [
    ("ui", os.path.join(OPEN_SANS, "OpenSans-Regular.ttf"), 16),
    ("brand", os.path.join(OPEN_SANS, "OpenSans-Light.ttf"), 34),
]


def build_font_atlas():
    first, last = 32, 127
    glyphs_per_size = {}
    atlas_width = 1024
    tiles = []
    y_cursor = 0
    for name, path, pixel_size in FONT_SIZES:
        font = ImageFont.truetype(path, pixel_size)
        ascent, descent = font.getmetrics()
        row_left = 0
        row_top = y_cursor
        row_height = 0
        entries = []
        for code in range(first, last):
            char = chr(code)
            tile = Image.new("RGBA", (pixel_size * 2 + 8, pixel_size * 2 + 8), (0, 0, 0, 0))
            tdraw = ImageDraw.Draw(tile)
            tdraw.text((2, ascent + 4), char, font=font, fill=(255, 255, 255, 255), anchor="ls")
            bbox = tile.getbbox()
            if bbox is None:
                advance = int(round(font.getlength(char)))
                entries.append((0, 0, 0, 0, advance, 0))
                continue
            x0, y0, x1, y1 = bbox
            glyph = tile.crop(bbox)
            advance = int(round(font.getlength(char)))
            ink_top = y0 - 4
            if row_left + glyph.width + 1 > atlas_width:
                y_cursor = row_top + row_height + 1
                row_left = 0
                row_top = y_cursor
                row_height = 0
            tiles.append((row_left, row_top, glyph))
            entries.append((row_left, row_top, glyph.width, glyph.height, advance, ink_top))
            row_left += glyph.width + 1
            row_height = max(row_height, glyph.height)
        glyphs_per_size[name] = entries
        y_cursor = row_top + row_height + 1
    atlas_height = y_cursor + 1
    atlas = Image.new("RGBA", (atlas_width, atlas_height), (0, 0, 0, 0))
    for x, y, glyph in tiles:
        atlas.paste(glyph, (x, y), glyph)
    alpha = atlas.split()[3]
    black = Image.new("RGBA", atlas.size, (0, 0, 0, 0))
    black.putalpha(alpha)
    atlas.save(os.path.join(FONTS, "atlas_white.png"))
    black.save(os.path.join(FONTS, "atlas_black.png"))

    out = []
    out.append("// @generated by crates/desktop/tools/gen_assets.py - do not edit.")
    out.append("")
    out.append("")
    out.append("pub const ATLAS_WHITE: &[u8] = include_bytes!(\"../assets/fonts/atlas_white.png\");")
    out.append("pub const ATLAS_BLACK: &[u8] = include_bytes!(\"../assets/fonts/atlas_black.png\");")
    out.append("pub const FIRST_CHAR: u32 = 32;")
    out.append("pub const LAST_CHAR: u32 = 127;")
    out.append("")
    out.append("#[derive(Clone, Copy)]")
    out.append("pub struct Glyph {")
    out.append("    pub atlas_x: u16,")
    out.append("    pub atlas_y: u16,")
    out.append("    pub w: u16,")
    out.append("    pub h: u16,")
    out.append("    pub advance: u16,")
    out.append("    pub ink_top: u16,")
    out.append("}")
    out.append("")
    out.append("pub const fn glyph_count() -> usize {")
    out.append("    (LAST_CHAR - FIRST_CHAR) as usize")
    out.append("}")
    for name, _, _ in FONT_SIZES:
        entries = glyphs_per_size[name]
        out.append("pub static GLYPHS_" + name.upper() + ": [Glyph; glyph_count()] = [")
        for (gx, gy, gw, gh, adv, ink_top) in entries:
            out.append(
                "    Glyph { atlas_x: %d, atlas_y: %d, w: %d, h: %d, advance: %d, ink_top: %d }," % (gx, gy, gw, gh, adv, ink_top)
            )
        out.append("];")
    out.append("")
    with open(os.path.join(SRC, "font_atlas_data.rs"), "w") as handle:
        handle.write(chr(10).join(out) + chr(10))


def main():
    make_wallpaper()
    make_logon_background()
    make_avatar()
    make_icons()
    make_logon_panel()
    make_taskbar()
    make_orb()
    make_brand_flag()
    make_buttons()
    build_font_atlas()
    print("assets written to", THEME, "and", FONTS)


if __name__ == "__main__":
    main()
