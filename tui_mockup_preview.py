# Scratch preview of the proposed minipro-tui layout.
# Renders the mockup from docs/TUI-PLAN.md as ANSI-colored text.
# Untracked throwaway — run it, look at it, delete it.
#
#   C:\Users\craig\miniconda3\python.exe tui_mockup_preview.py
#
import os
import re
import shutil
import sys

# ── Terminal setup ────────────────────────────────────────────────────────────
if os.name == "nt":
    # Enable VT100 processing on legacy conhost; Windows Terminal already has it.
    import ctypes
    h = ctypes.windll.kernel32.GetStdHandle(-11)
    mode = ctypes.c_ulong()
    ctypes.windll.kernel32.GetConsoleMode(h, ctypes.byref(mode))
    ctypes.windll.kernel32.SetConsoleMode(h, mode.value | 0x0004)

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

ANSI_RE = re.compile(r"\x1b\[[0-9;]*m")


def vlen(s: str) -> int:
    return len(ANSI_RE.sub("", s))


def pad(s: str, w: int) -> str:
    return s + " " * max(0, w - vlen(s))


# ── Palette (truecolor) ───────────────────────────────────────────────────────
def fg(r, g, b):
    return f"\x1b[38;2;{r};{g};{b}m"


def bg(r, g, b):
    return f"\x1b[48;2;{r};{g};{b}m"


RST = "\x1b[0m"
BOLD = "\x1b[1m"
DIM = "\x1b[2m"

C = {
    "border": fg(76, 82, 102),
    "txt":    fg(200, 205, 222),
    "dim":    fg(112, 120, 145),
    "faint":  fg(88, 95, 118),
    "accent": fg(126, 162, 247),   # blue
    "acc2":   fg(187, 154, 247),   # purple
    "green":  fg(90, 205, 145),
    "red":    fg(242, 96, 96),
    "amber":  fg(232, 184, 90),
    "cyan":   fg(96, 200, 210),
    "selbg":  bg(44, 55, 100),     # selected row
    "keybg":  bg(32, 37, 56),      # keybar strip
    "proge":  fg(52, 58, 80),      # progress empty
}


def T(s):      return C["txt"] + s + RST
def Dm(s):     return C["dim"] + s + RST
def Ft(s):     return C["faint"] + s + RST
def Ac(s):     return C["accent"] + s + RST
def AcB(s):    return C["accent"] + BOLD + s + RST
def P2(s):     return C["acc2"] + s + RST
def Gr(s):     return C["green"] + s + RST
def Rd(s):     return C["red"] + s + RST
def Am(s):     return C["amber"] + s + RST
def Cy(s):     return C["cyan"] + s + RST
def Bo(s):     return BOLD + s + RST
def Sel(s):    return C["selbg"] + C["txt"] + s + RST
def Brd(s):    return C["border"] + s + RST
def Dpl(s):    return bg(187,154,247) + fg(40,44,64) + s + RST   # ● dimple on chip


# ── Frame geometry ────────────────────────────────────────────────────────────
LW, CW, RW = 26, 56, 34            # pane inner widths
INNER = LW + CW + RW + 2           # + 2 column separators
V = Brd("│")
H = Brd("─")


def row(*cells):
    """Compose one interior frame line from pane strings."""
    return V + V.join(pad(c, w) for c, w in zip(cells, (LW, CW, RW))) + V


def sep():
    return Brd("├" + "─" * LW + "┬" + "─" * CW + "┬" + "─" * RW + "┤")


def sel_row(text, w):
    return C["selbg"] + text.ljust(w) + RST


# ── Left pane: DEVICES ────────────────────────────────────────────────────────
L = []
L.append(" " + AcB("DEVICES"))
L.append("  " + Brd("╭" + "─" * (LW - 5) + "╮"))
L.append("  " + Brd("│") + Ac("/") + T("25Q64") + Ac("▌") + " " * (LW - 12) + Brd("│"))
L.append("  " + Brd("╰" + "─" * (LW - 5) + "╯"))
L.append(" " + Ft("★ FAVORITES"))
L.append(" " + Ac("★") + " " + T("AT28C256") + Dm("     Atmel"))
L.append(" " + Ac("★") + " " + T("PIC16F628A") + Dm("  Microchip"))
L.append(" " + Ft("─" * (LW - 2)))
L.append(" " + Dm("27 results") + Ft("scroll ↓".rjust(LW - 1 - len("27 results"))))
for name, mfg, sel in [
    ("W25Q32JV",   "Winbond", False),
    ("W25Q64BV",   "Winbond", False),
    ("W25Q64JV",   "Winbond", True),
    ("W25Q64JV-M", "Winbond", False),
    ("W25Q64FV",   "Winbond", False),
    ("GD25Q64C",   "GigaDev", False),
    ("MX25L6433F", "Macronix", False),
]:
    line = f"   {name:<13}{mfg}"
    if sel:
        L.append(" " + sel_row(f" ▶{name:<14}{mfg}", LW - 1))
    else:
        L.append("   " + T(f"{name:<14}") + Dm(mfg))
L.append(" " + Ft("─" * (LW - 2)))
L.append(" " + Dm("Selected"))
L.append(" " + Bo("W25Q64JV"))
L.append(" " + Dm("Winbond · SPI NOR"))
L.append(" " + Dm("8 MB code · SOIC-8"))
L.append(" " + Dm("VCC 3.3V · Chip ID ✓"))
L.append(" " + Ft("─" * (LW - 2)))
L.append(" " + Dm("Diagnostics") + Ft(" ▸"))

# ── Center pane: OPERATION ────────────────────────────────────────────────────
Ce = []
Ce.append(" " + Dm("Read") + "  " + C["selbg"] + Bo(" Write ") + RST + "  " + Dm("Verify  Erase  Blank  ID  Logic  Config"))
Ce.append("")
Ce.append(" " + Dm("Interface") + "    " + T("ZIF socket") + Ft("        ▾"))
Ce.append(" " + Dm("Page") + "         " + T("Code") + Ft("              ▾"))
Ce.append(" " + Dm("Format") + "       " + T("Auto") + Ft("              ▾"))
Ce.append(" " + Dm("Size diff") + "    " + T("Error") + Ft("             ▾"))
Ce.append("")
Ce.append(" " + Gr("[x]") + " " + T("Chip ID check") + "      " + Gr("[x]") + " " + T("Pin contact check"))
Ce.append(" " + Ft("[ ]") + " " + T("Skip erase") + "         " + Gr("[x]") + " " + T("Unprotect before"))
Ce.append(" " + Ft("[ ]") + " " + T("Skip verify") + "        " + Ft("[ ]") + " " + T("Re-protect after"))
Ce.append(" " + Ft("[ ]") + " " + T("Skip blank pages"))
Ce.append("")
Ce.append(" " + Dm("▸ Advanced") + Ft("  (VPP / VCC / VDD overrides)"))
Ce.append(" " + Ft("─" * (CW - 2)))
Ce.append(" " + Dm("Buffer") + "  " + T("W25Q64JV_dump.bin") + Ft("  · 8,388,608 B"))
Ce.append(" " + Dm("       ") + "  " + Dm("CRC-32") + " " + Cy("A1B2C3D4") + Ft("  · format bin"))
Ce.append(" " + Ft("─" * (CW - 2)))
Ce.append(" " + AcB("[f]") + " " + T("write from file") + "    " + AcB("[b]") + " " + T("write buffer"))
Ce.append("")
filled, empty = 21, 8
bar = Ac("█" * filled) + C["proge"] + "░" * empty + RST
Ce.append(" " + Am("◐") + " " + Bo("Writing") + "   " + bar + "  " + Bo("72%"))
Ce.append(" " * 11 + Dm("6.0 MB / 8.0 MB · 12.4s"))
Ce.append("")
Ce.append("")

# ── Right pane: LOG + ZIF ─────────────────────────────────────────────────────
R = []
R.append(" " + AcB("LOG"))
R.append(" " + Ft("09:12:04") + " " + Gr("●") + " " + T("TL866II+ connected"))
R.append(" " + Ft("09:12:31") + " " + Dm("device W25Q64JV"))
R.append(" " + Ft("09:13:02") + " " + T("Chip ID OK") + " " + Cy("0xEF4017"))
R.append(" " + Ft("09:13:02") + " " + T("Writing 8388608 B…"))
R.append(" " + Ft("09:13:09") + " " + Am("OVC check ok"))
R.append(" " + Ft("09:13:14") + " " + Dm("…"))
R.append(" " + Ft("─" * (RW - 2)))
R.append(" " + AcB("ZIF-40") + Ft(" · DIP8 · top-justified"))
# 40-pin ZIF, DIP8 top-justified → chip occupies ZIF pins 1-4 + 37-40.
# Each occupied row draws the IC's pin as a ─ stroke bridging the slot
# cell to the chip wall (┤/├ tee joins) — pin↔ZIF alignment is drawn, not
# implied. Chip glyph: ∪ notch at its pin-1 end and a ● dimple painted
# inside the body at the pin-1 corner (cell bg = chip colour, else the
# mark punches a hole in the fill). Socket frame plain — printed pin
# numbers already identify ZIF pin 1.
# Bad contact: red cell+stub + red arrow hugging that pin's own label
# column (left pins "N►", right pins "◄N") — never a far-end callout.
# Lever = ═ bar flush against the socket's lever end — top on every model
# except T48, where it moves below └───────────┘.
occupied = {1, 2, 3, 4, 37, 38, 39, 40}
bad_pin = 3
chip_top, chip_bot = 1, 4   # ZIF rows carrying chip pins; edges extend
                            # one row past the outermost pins, like a
                            # real package — top edge uses the socket's
                            # top margin row, bottom edge shares pin 5/36
R.append("    " + Ft("═" * 13) + " " + Dm("lever"))
R.append("    " + Brd("┌" + "─" * 11 + "┐"))
R.append("    " + Brd("│") + "  " + P2("╭──∪──╮") + "  " + Brd("│"))
for lp, rp in zip(range(1, 6), range(40, 34, -1)):
    lbad, rbad = lp == bad_pin, rp == bad_pin
    llab = Rd(f"{lp:>2}") if lbad else Dm(f"{lp:>2}")
    lsep = Rd("►") if lbad else " "
    rsep = Rd("◄") if rbad else " "
    rlab = Rd(str(rp)) if rbad else Dm(str(rp))
    lc = Rd("▓") if lbad else (Gr("▓") if lp in occupied else Ft("·"))
    rc = Rd("▓") if rbad else (Gr("▓") if rp in occupied else Ft("·"))
    lstub = (Rd if lbad else Gr)("─") if lp in occupied else " "
    rstub = (Rd if rbad else Gr)("─") if rp in occupied else " "
    if lp == chip_bot + 1:      mid = P2("╰─────╯")
    elif lp == chip_top:        mid = P2("┤") + Dpl("●") + P2("████├")
    else:                       mid = P2("┤█████├")
    R.append(" " + llab + lsep + Brd("│") + lc + lstub + mid + rstub + rc + Brd("│") + rsep + rlab)
R.append(" " + Ft(" ⋮") + " " + Brd("│") + Ft("⋮") + " " * 9 + Ft("⋮") + Brd("│") + " " + Ft("⋮"))
lp, rp = 20, 21
R.append(" " + Dm(f"{lp:>2}") + " " + Brd("│") + Ft("·") + " " * 9 + Ft("·") + Brd("│") + " " + Dm(str(rp)))
R.append("    " + Brd("│") + " " * 11 + Brd("│"))
R.append("    " + Brd("└" + "─" * 11 + "┘"))
R.append(" " + Ft("─" * (RW - 2)))
R.append(" " + Rd("✗") + " " + T("Pin test:") + " " + Rd("bad contact pin 3"))

# ── Compose frame ─────────────────────────────────────────────────────────────
rows = max(len(L), len(Ce), len(R))
L += [""] * (rows - len(L))
Ce += [""] * (rows - len(Ce))
R += [""] * (rows - len(R))

lines = []
title = "─ " + Bo("minipro-rs") + " " + Dm("v0.9.0") + " "
status = Ac("●") + " " + Gr("TL866II+") + Dm(" · FW 04.2.86 · S/N 5A3F") + " "
right = " " + Dm("[?] help") + " "
gap = INNER - vlen(title) - vlen(status) - vlen(right)
lines.append(Brd("╭") + title + status + Brd("─" * max(0, gap)) + right + Brd("╮"))
lines.append(Brd("├" + "─" * LW + "┬" + "─" * CW + "┬" + "─" * RW + "┤"))
for i in range(rows):
    lines.append(row(L[i], Ce[i], R[i]))
lines.append(Brd("├" + "─" * INNER + "┤"))
keybar = (
    " " + AcB("⇥") + Dm(" panes") + Ft("  ·  ") + AcB("1-8") + Dm(" op")
    + Ft("  ·  ") + AcB("Space") + Dm(" toggle") + Ft("  ·  ") + AcB("←→")
    + Dm(" change") + Ft("  ·  ") + AcB("Enter") + Dm(" run") + Ft("  ·  ")
    + AcB("/") + Dm(" search") + Ft("  ·  ") + AcB("q") + Dm(" quit") + " "
)
lines.append(V + C["keybg"] + pad(keybar + RST + C["keybg"], INNER) + RST + V)
lines.append(Brd("╰" + "─" * INNER + "╯"))

out = "\n".join(lines)
width = vlen(lines[0])

tw = shutil.get_terminal_size().columns
if tw < width + 2:
    print(Am(f"(terminal is {tw} cols; mockup needs {width + 2} — widen or zoom out)\n"))

print(out)
print()
print(Dm("static mockup — proposed layout from docs/TUI-PLAN.md"))
