# Mimics the launcher's frame diffs: ~800 scattered per-cell RGB updates
# per frame at 10fps, no raw mode, no alternate screen. Isolates whether
# the terminal itself can render this class of output.
import random
import sys
import time

w = sys.stdout.write
w("\x1b[2J")
random.seed(1)
for frame in range(100):
    parts = []
    for _ in range(800):
        x = random.randint(1, 190)
        y = random.randint(1, 42)
        r, g, b = (random.randint(0, 255) for _ in range(3))
        parts.append(f"\x1b[{y};{x}H\x1b[38;2;{r};{g};{b}m█")
    w("".join(parts))
    sys.stdout.flush()
    time.sleep(0.1)
w("\x1b[0m\x1b[43;1HSPAM DONE\n")
