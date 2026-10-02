"""Writes icon-source.png (1024x1024) for `npx tauri icon`."""
import struct
import zlib

SIZE = 1024
BG, FG = (22, 24, 29), (122, 162, 247)

def pixel(x: int, y: int) -> tuple[int, int, int]:
    cx = cy = SIZE / 2
    r = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
    return FG if 300 < r < 400 or (abs(x - cx) < 50 and abs(y - cy) < 300) else BG

rows = b"".join(b"\x00" + bytes(c for x in range(SIZE) for c in pixel(x, y)) for y in range(SIZE))
def chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
open("icon-source.png", "wb").write(png)
