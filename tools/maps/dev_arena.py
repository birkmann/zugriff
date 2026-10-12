"""dev_arena: a courtyard for testing movement, weapons and bots (not one of the three real maps).

North: KSK spawn. South: OPFOR spawn. West: site A (crates). East: site B (a two-storey house with stairs and a window).
Mid: a ramp onto a platform and crates of 32, 48 and 64 units (jump, crouch-jump heights).
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from zmap import Map, box, ramp, stairs  # noqa: E402

m = Map("dev_arena")
S = 1024  # half size of the yard
H = 384  # wall height
T = 32  # wall thickness

# Ground and outer walls (open sky).
m.block((-S - T, -S - T, -32), (S + T, S + T, 0), {"top": "dev/ground", "all": "dev/ground"})
# Real material test: CC0 bricks (ambientCG Bricks090), 1.6 m per tile.
brick = {"all": "mat/bricks090", "top": "dev/trim"}
m.block((-S - T, -S - T, 0), (-S, S + T, H), brick, scale=0.125)
m.block((S, -S - T, 0), (S + T, S + T, H), brick, scale=0.125)
m.block((-S, -S - T, 0), (S, -S, H), brick, scale=0.125)
m.block((-S, S, 0), (S, S + T, H), brick, scale=0.125)
# Keep players inside: a clip lid at the wall tops.
m.block((-S, -S, H), (S, S, H + 16), "tools/clip")

# Mid: ramp up to a platform, and a row of test crates.
m.add(ramp((-192, -64, 0), (64, 64, 96), "+x", "dev/concrete"))
m.block((64, -64, 0), (192, 64, 96), {"top": "dev/concrete", "all": "dev/trim"})
for i, h in enumerate((32, 48, 64, 72)):
    x = -448 + i * 96
    m.block((x, 192, 0), (x + 64, 256, h), "dev/crate")

# Site A (west): a crate stack.
for lo, hi in [((-800, -96, 0), (-736, -32, 64)), ((-736, -96, 0), (-672, -32, 64)), ((-800, -32, 0), (-736, 32, 64)),
               ((-768, -80, 64), (-704, -16, 128)), ((-640, 96, 0), (-576, 160, 48))]:
    m.block(lo, hi, "dev/crate")
m.volume("func_bomb_target", (-896, -256, 0), (-512, 256, 128), site="A")

# Site B (east): a two-storey house, doors on west and south, stairs inside, a window upstairs.
x0, x1, y0, y1 = 448, 896, -256, 192
floor2 = 160
roof = 320
w = 16
wall = {"sides": "dev/plaster", "top": "dev/trim", "bottom": "dev/trim"}
m.block((x1 - w, y0, 0), (x1, y1, roof), wall)  # east
m.block((x0 + w, y1 - w, 0), (x1 - w, y1, roof), wall)  # north
# West wall with a door (y -32..32, 0..112).
m.block((x0, y0, 0), (x0 + w, -32, roof), wall)
m.block((x0, 32, 0), (x0 + w, y1, roof), wall)
m.block((x0, -32, 112), (x0 + w, 32, roof), wall)
# South wall with a door (x 576..640) and a window upstairs (x 720..816, z 208..272).
m.block((x0 + w, y0, 0), (576, y0 + w, roof), wall)
m.block((640, y0, 0), (720, y0 + w, roof), wall)
m.block((576, y0, 112), (640, y0 + w, roof), wall)
m.block((720, y0, 0), (816, y0 + w, 208), wall)
m.block((720, y0, 272), (816, y0 + w, roof), wall)
m.block((816, y0, 0), (x1 - w, y0 + w, roof), wall)
m.brush_entity("func_glass", box((720, y0 + 6, 208), (816, y0 + 10, 272), "dev/glass"))
# Upper floor (with a stairwell gap in the north-west) and the roof.
m.block((x0 + w, y0 + w, floor2), (x1 - w, y1 - 176, floor2 + 12), {"all": "dev/floor", "bottom": "dev/ceiling"})
m.block((x0 + w + 240, y1 - 176, floor2), (x1 - w, y1 - w, floor2 + 12), {"all": "dev/floor", "bottom": "dev/ceiling"})
m.block((x0, y0, roof), (x1, y1, roof + 16), {"all": "dev/trim", "bottom": "dev/ceiling"})
# Stairs up along the north wall, rising to the east.
m.add(stairs((x0 + w, y1 - 176 + 16, 0), (x0 + w + 240, y1 - w, floor2 + 12), "+x", 10, "dev/concrete"))
m.volume("func_bomb_target", (x0 + w, y0 + w, 0), (x1 - w, y1 - w, floor2), site="B")

# Spawns and buy zones.
for i in range(5):
    m.point("info_player_ksk", (-256 + i * 128, 800, 4), angle=270)
    m.point("info_player_opfor", (-256 + i * 128, -800, 4), angle=90)
m.volume("func_buyzone", (-512, 640, 0), (512, S, 128), team="ksk")
m.volume("func_buyzone", (-512, -S, 0), (512, -640, 128), team="opfor")

# Callouts.
m.volume("zg_place", (-S, 512, 0), (S, S, H), name="Nord")
m.volume("zg_place", (-S, -S, 0), (S, -512, H), name="Süd")
m.volume("zg_place", (-512, -512, 0), (448, 512, H), name="Mitte")
m.volume("zg_place", (-S, -512, 0), (-512, 512, H), name="A")
m.volume("zg_place", (448, -512, 0), (S, 512, H), name="B")

# Sun and sky for the renderer.
m.point("light_environment", (0, 0, 256), pitch="-50", yaw="35", color="255 240 220", ambient="120 135 160")

m.save(os.path.join(os.path.dirname(__file__), "../../assets/maps/dev_arena.map"))
