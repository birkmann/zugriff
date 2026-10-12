import json, os, subprocess, sys
C = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".cache") + ""
T = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "assets", "textures")
BIN = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "..", "..", "..", "target", "release", "zg-tex")
jobs = json.load(open(C + "/jobs.json"))
only = sys.argv[1:]
for name, j in jobs.items():
    if only and name not in only: continue
    d = f"{T}/{name}"; os.makedirs(d, exist_ok=True)
    for args in (["color", j["color"], f"{d}/color.ktx2"], ["normal", j["normal"], f"{d}/normal.ktx2"],
                 ["orm", j["ao"], j["rough"], j["metal"], f"{d}/orm.ktx2"]):
        r = subprocess.run([BIN] + args, capture_output=True, text=True)
        if r.returncode: print("FAIL", name, args, r.stderr); sys.exit(1)
        print(r.stdout.strip())
