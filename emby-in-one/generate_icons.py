#!/usr/bin/env python3
"""Generate Emby-style app icons: rounded square green bg + white bold 'e'."""
from PIL import Image, ImageDraw, ImageFont
import shutil, subprocess

EMBY_GREEN = (82, 181, 75, 255)  # #52B54B
WHITE = (255, 255, 255, 255)
FONT = '/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf'

def make_icon(size):
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    radius = size // 6
    draw.rounded_rectangle([0, 0, size-1, size-1], radius=radius, fill=EMBY_GREEN)
    font_size = int(size * 0.62)
    font = ImageFont.truetype(FONT, font_size)
    bbox = draw.textbbox((0, 0), 'e', font=font)
    text_w = bbox[2] - bbox[0]
    text_h = bbox[3] - bbox[1]
    x = (size - text_w) // 2 - bbox[0]
    y = (size - text_h) // 2 - bbox[1] - int(size * 0.02)
    draw.text((x, y), 'e', fill=WHITE, font=font)
    return img

import sys
BASE = sys.argv[1] if len(sys.argv) > 1 else '.'

for size, name in [(256, 'icon-256.png'), (64, 'icon-64.png')]:
    img = make_icon(size)
    img.save(f'{BASE}/ui/images/{name}', 'PNG')
    shutil.copy2(f'{BASE}/ui/images/{name}', f'{BASE}/app/ui/images/{name}')
    print(f'Generated {name} ({size}x{size})')

# Copy to root ICON files
shutil.copy2(f'{BASE}/ui/images/icon-256.png', f'{BASE}/ICON_256.PNG')
shutil.copy2(f'{BASE}/ui/images/icon-64.png', f'{BASE}/ICON.PNG')
print('Copied ICON_256.PNG and ICON.PNG')

# Verify md5 != hyatlas
HYATLAS = {'icon-256.png': '55148d690e2d7adb9cc3e2aa82b075d0',
           'icon-64.png': 'ba898018ef1d6514459545d7f8bc0092'}
for name, bad_md5 in HYATLAS.items():
    r = subprocess.run(['md5sum', f'{BASE}/ui/images/{name}'], capture_output=True, text=True)
    md5 = r.stdout.split()[0]
    ok = 'OK' if md5 != bad_md5 else 'COLLIDES!'
    print(f'{name}: {md5} ({ok})')
