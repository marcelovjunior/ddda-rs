"""Record this machine's Direct3D 9 adapter answers as the runtime's
capability profile (crates/ddruntime/data/d3d9_profile.txt).

The game's device set-up (0x00DB1480 and its helpers 0x00DA9EC0,
0x00DB0DE0) asks the IDirect3D9 object for the adapter identifier, the
device caps and many format/multisample/depth-stencil checks, and takes
paths depending on the answers. The runtime answers from a profile
recorded from a real driver instead of made-up values, so the answers are
deterministic and plausible together. Only the adapter queries are
recorded (no device is created, no window needed).

Usage: python scripts/d3d9_profile.py [out]

Format, one record per line (hex numbers, '#' comments):
  identifier <2200 hex digits: D3DADAPTER_IDENTIFIER9, 1100 bytes>
  caps <608 hex digits: D3DCAPS9, 304 bytes>      IDirect3D9::GetDeviceCaps
  devicecaps <608 hex digits>                     IDirect3DDevice9::GetDeviceCaps
  texturememory <bytes>                           IDirect3DDevice9::GetAvailableTextureMem
  renderstate <state> <value>                     a fresh device's GetRenderState
  samplerstate <sampler> <state> <value>          ... GetSamplerState (sampler 0, vertex sampler 257)
  stagestate <stage> <state> <value>              ... GetTextureStageState (stages 0, 1)
  format <usage> <rtype> <hresult> <format>,<format>,...
                                                 CheckDeviceFormat(HAL, X8R8G8B8)
  type <backbuffer format> <windowed> <hresult>  CheckDeviceType(HAL, X8R8G8B8)
  multisample <format> <windowed> <type> <hresult> <quality levels>
  depth <render target format> <depth format> <hresult>
  conversion <source> <target> <hresult>
  default <kind> <hresult>
For each kind, the most common result is written once as its default and
only the other answers are listed (most format checks answer
D3DERR_INVALIDCALL). CheckDeviceFormat usages that were not recorded at all
are reported by the runtime rather than guessed.
"""
import ctypes
import sys
from ctypes import POINTER, byref, c_long, c_uint32, c_void_p

HAL = 1
X8R8G8B8 = 22


def fourcc(s):
    return int.from_bytes(s.encode(), "little")


FORMATS = [
    20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 40, 41,
    50, 51, 52, 60, 61, 62, 63, 64, 67, 70, 71, 73, 75, 77, 79, 80, 81, 82, 83,
    84, 85, 100, 101, 102, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
] + [fourcc(s) for s in (
    "UYVY", "RGBG", "YUY2", "GRGB", "DXT1", "DXT2", "DXT3", "DXT4", "DXT5",
    "MET1", "INTZ", "RAWZ", "DF16", "DF24", "NULL", "ATI1", "ATI2", "RESZ",
    "ATOC", "SSAA", "NVDB", "R2VB", "INST", "A2M0", "A2M1", "AL16", "AR16",
    "R16 ", "L16 ", "NV12", "YV12",
)]
DEPTH = [70, 71, 73, 75, 77, 79, 80, 82, 83, 84, 85] + [fourcc(s) for s in ("INTZ", "RAWZ", "DF16", "DF24")]
RTYPES = [1, 2, 3, 4, 5, 6, 7]  # surface, volume, texture, volume texture, cube, vertex/index buffer
BASE_USAGES = [0, 0x1, 0x2, 0x200, 0x400, 0x401, 0x600, 0x201]
QUERY_USAGES = [0, 0x8000, 0x10000, 0x20000, 0x40000, 0x80000, 0x100000, 0x200000]
BACKBUFFERS = [20, 21, 22, 23, 24, 25, 26, 30, 31, 35]


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "crates/ddruntime/data/d3d9_profile.txt"
    d3d9 = ctypes.WinDLL("d3d9")
    d3d9.Direct3DCreate9.restype = c_void_p
    d3d = d3d9.Direct3DCreate9(32)
    assert d3d, "Direct3DCreate9 failed"
    vtbl = ctypes.cast(ctypes.cast(d3d, POINTER(c_void_p))[0], POINTER(c_void_p))

    def method(slot, *argtypes):
        f = ctypes.WINFUNCTYPE(c_long, c_void_p, *argtypes)(vtbl[slot])
        return lambda *a: f(d3d, *a) & 0xFFFFFFFF

    U = c_uint32
    get_identifier = method(5, U, U, c_void_p)
    check_type = method(9, U, U, U, U, U)
    check_format = method(10, U, U, U, U, U, U)
    check_ms = method(11, U, U, U, U, U, POINTER(U))
    check_depth = method(12, U, U, U, U, U)
    check_conv = method(13, U, U, U, U)
    get_caps = method(14, U, U, c_void_p)

    lines = ["# Direct3D 9 adapter profile recorded by scripts/d3d9_profile.py"]
    ident = ctypes.create_string_buffer(1100)
    assert get_identifier(0, 0, ident) == 0
    desc = ident.raw[512:1024].split(b"\0")[0].decode(errors="replace")
    lines.append(f"# adapter: {desc}")
    lines.append("identifier " + ident.raw.hex())
    caps = ctypes.create_string_buffer(304)
    assert get_caps(0, HAL, caps) == 0
    lines.append("caps " + caps.raw.hex())

    records = {}  # kind -> [(fields, hresult, extra)]

    def record(kind, fields, hr, extra=()):
        records.setdefault(kind, []).append((fields, hr, extra))

    for bb in BACKBUFFERS:
        for windowed in (0, 1):
            record("type", (bb, windowed), check_type(0, HAL, X8R8G8B8, bb, windowed))
    for base in BASE_USAGES:
        for q in QUERY_USAGES:
            usage = base | q
            for rtype in RTYPES:
                for fmt in FORMATS:
                    record("format", (usage, rtype, fmt), check_format(0, HAL, X8R8G8B8, usage, rtype, fmt))
    for fmt in FORMATS:
        for windowed in (0, 1):
            for ms in range(17):
                quality = U(0)
                hr = check_ms(0, HAL, fmt, windowed, ms, byref(quality))
                record("multisample", (fmt, windowed, ms), hr, (quality.value,))
    for rt in FORMATS:
        for ds in DEPTH:
            record("depth", (rt, ds), check_depth(0, HAL, X8R8G8B8, rt, ds))
    for src in FORMATS:
        for tgt in BACKBUFFERS:
            record("conversion", (src, tgt), check_conv(0, HAL, src, tgt))

    for kind, rows in records.items():
        # the default is the most common answer; rows with quality levels
        # (multisample) are always listed
        codes = [hr for _, hr, _ in rows]
        default = max(set(codes), key=codes.count)
        lines.append(f"default {kind} {default:x}")
        if kind == "format":
            groups = {}
            for (usage, rtype, fmt), hr, _ in rows:
                if hr != default:
                    groups.setdefault((usage, rtype, hr), []).append(f"{fmt:x}")
            for (usage, rtype, hr), fmts in groups.items():
                lines.append(f"format {usage:x} {rtype:x} {hr:x} {','.join(fmts)}")
            continue
        for fields, hr, extra in rows:
            if hr != default or any(extra):
                lines.append(" ".join([kind] + [f"{v:x}" for v in fields + (hr,) + extra]))

    # A device, created the way 0x00DB1480 creates it (hardware vertex
    # processing, FPU preserve, multithreaded; windowed 1280x720 A8R8G8B8 on
    # a hidden window), for the answers only a device gives.
    user32 = ctypes.WinDLL("user32")
    user32.CreateWindowExW.restype = c_void_p
    user32.CreateWindowExW.argtypes = [U, ctypes.c_wchar_p, ctypes.c_wchar_p, U, ctypes.c_int, ctypes.c_int,
                                       ctypes.c_int, ctypes.c_int, c_void_p, c_void_p, c_void_p, c_void_p]
    hwnd = user32.CreateWindowExW(0, "STATIC", "d3d9 profile", 0x00CA0000, 0, 0, 1286, 749, None, None, None, None)
    assert hwnd, "CreateWindowExW failed"
    # x64 D3DPRESENT_PARAMETERS has an 8-byte HWND at offset 32 (x86: 4 bytes at 28)
    pp64 = ctypes.create_string_buffer(64)
    fields = [(0, 1280), (4, 720), (8, 21), (12, 1), (16, 0), (20, 0), (24, 1)]
    for off, v in fields:
        ctypes.memmove(ctypes.addressof(pp64) + off, ctypes.byref(U(v)), 4)
    ctypes.memmove(ctypes.addressof(pp64) + 32, ctypes.byref(c_void_p(hwnd)), 8)
    for off, v in [(40, 1), (44, 0), (48, 0), (52, 0), (56, 0), (60, 0x80000000)]:
        ctypes.memmove(ctypes.addressof(pp64) + off, ctypes.byref(U(v)), 4)
    create_device = method(16, U, U, c_void_p, U, c_void_p, POINTER(c_void_p))
    dev = c_void_p()
    hr = create_device(0, HAL, hwnd, 0x46, pp64, byref(dev))
    assert hr == 0 and dev.value, f"CreateDevice failed: {hr:#x}"
    dvtbl = ctypes.cast(ctypes.cast(dev.value, POINTER(c_void_p))[0], POINTER(c_void_p))
    texture_mem = ctypes.WINFUNCTYPE(U, c_void_p)(dvtbl[4])(dev.value)
    lines.append(f"texturememory {texture_mem:x}")
    dcaps = ctypes.create_string_buffer(304)
    assert ctypes.WINFUNCTYPE(c_long, c_void_p, c_void_p)(dvtbl[7])(dev.value, dcaps) == 0
    lines.append("devicecaps " + dcaps.raw.hex())
    # the fresh device's state: GetRenderState (slot 58) for every state
    # type the device accepts, GetSamplerState (slot 68) for sampler 0 and
    # vertex sampler 257, GetTextureStageState (slot 66) for stage 0 and 1
    get_rs = ctypes.WINFUNCTYPE(c_long, c_void_p, U, POINTER(U))(dvtbl[58])
    get_ss = ctypes.WINFUNCTYPE(c_long, c_void_p, U, U, POINTER(U))(dvtbl[68])
    get_ts = ctypes.WINFUNCTYPE(c_long, c_void_p, U, U, POINTER(U))(dvtbl[66])
    for state in range(256):
        v = U(0)
        if get_rs(dev.value, state, byref(v)) == 0:
            lines.append(f"renderstate {state:x} {v.value:x}")
    for sampler in (0, 257):
        for state in range(1, 14):
            v = U(0)
            if get_ss(dev.value, sampler, state, byref(v)) == 0:
                lines.append(f"samplerstate {sampler:x} {state:x} {v.value:x}")
    for stage in (0, 1):
        for state in range(1, 33):
            v = U(0)
            if get_ts(dev.value, stage, state, byref(v)) == 0:
                lines.append(f"stagestate {stage:x} {state:x} {v.value:x}")
    ctypes.WINFUNCTYPE(c_uint32, c_void_p)(dvtbl[2])(dev.value)
    user32.DestroyWindow.argtypes = [c_void_p]
    user32.DestroyWindow(hwnd)

    # IDirect3D9::Release
    ctypes.WINFUNCTYPE(c_uint32, c_void_p)(vtbl[2])(d3d)
    with open(out, "w", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{out}: {len(lines)} lines; adapter {desc}")


if __name__ == "__main__":
    main()
