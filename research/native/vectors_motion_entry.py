"""Vectors for the motion_entry pattern (patterns.py motion_entry). Private
output. The species base entry runs for real (allowed), with the getDTI
fixture and its class-id global; the owner's vtable +0x90 is a capture
fixture that stores the six stack arguments and ECX, then `ret 0x18`."""
import json, os, random, struct, sys
sys.path.insert(0, os.path.dirname(__file__))
import harness as h
N = "E:/ddda-re/notes/native"; S = h.SCRATCH; rnd = random.Random(47)
specs = json.load(open(f"{N}/patterns/motion_entry.json"))
fam = json.load(open("E:/ddda-re/notes/inventory/exact_families.json"))
base_global = {0x008D4840: 0x01996BC4,  # goblin; then the species bases (ddnative::species)
               0x00A4DB70: 0x0199F320, 0x008FBDD0: 0x01997F64, 0x00919DB0: 0x0199886C, 0x0095A810: 0x0199A36C,
               0x00A2AE80: 0x0199EE38, 0x00A202E0: 0x0199E9D8, 0x009C5890: 0x0199CEA4, 0x009B4920: 0x0199C82C,
               0x009BD970: 0x0199CD44}
for f in fam:
    if f["rep"] == 0x004E0EB0:
        for m in f["members"]:
            base_global[m["addr"]] = m["params"][1]
OWN, VT, DTIF, CAPF, DTI, CAP = 0x1000, 0x100, 0x180, 0x1C0, 0x200, 0x300
cap_code = b"".join(bytes([0x8B, 0x44, 0x24, 4 + 4 * i, 0xA3]) + struct.pack("<I", S + CAP + 4 * i) for i in range(6))
cap_code += bytes([0x89, 0x0D]) + struct.pack("<I", S + CAP + 24) + bytes([0xC2, 0x18, 0x00])
cases = []
for sp in specs:
    g = base_global.get(sp["base"])
    if g is None:
        continue
    for _ in range(3):
        img = bytearray(rnd.getrandbits(8) for _ in range(0x9000))
        struct.pack_into("<I", img, 0x44, S + OWN); struct.pack_into("<I", img, OWN, S + VT)
        struct.pack_into("<I", img, VT + 0x10, S + DTIF); struct.pack_into("<I", img, VT + 0x90, S + CAPF)
        img[DTIF:DTIF + 6] = bytes([0xB8]) + struct.pack("<I", S + DTI) + bytes([0xC3])
        img[CAPF:CAPF + len(cap_code)] = cap_code
        cid = rnd.getrandbits(32) | 1
        struct.pack_into("<I", img, DTI + 4, cid); struct.pack_into("<I", img, DTI + 0x10, 0)
        struct.pack_into("<I", img, OWN + 0x206C, rnd.choice([0, 0x80, 0x82]))
        # species tails follow these: keep them valid (no parts, no sub-object)
        struct.pack_into("<I", img, OWN + 0x60E8, 0); struct.pack_into("<I", img, OWN + 0x7324, 0)
        fields = {str(x): struct.unpack_from("<I", img, x)[0] for kind, x in sp["args"] if kind == "field"}
        r = h.run(sp["addr"], ecx=S, scratch=bytes(img), allow={sp["base"], 0x007F1890},
                  poke={g: cid, 0x018CFE80: 0, 0x018CFE84: 0, 0x018CFE88: 0})
        assert r["esp_delta"] == 0
        cap = list(struct.unpack_from("<7I", r["scratch"], CAP))
        cases.append({"addr": sp["addr"], "args": sp["args"], "fields": fields, "captured": cap[:6], "this": cap[6],
                      "owner": S + OWN})
json.dump(cases, open(f"{N}/vectors/motion_entry.json", "w"))
print(len(cases), "cases over", len({c['addr'] for c in cases}), "functions")
