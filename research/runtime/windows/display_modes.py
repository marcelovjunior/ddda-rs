"""Compare the host's Direct3D 9 adapter modes with user32's display modes
(brief L34), read-only.

The game builds its resolution list from IDirect3D9::GetAdapterModeCount /
EnumAdapterModes (adapter 0, D3DFMT_X8R8G8B8) and its desktop from
GetAdapterDisplayMode. This prints both D3D9's answers and what
EnumDisplaySettingsW reports for the same monitor, and whether D3D9's list
is EnumDisplaySettingsW's 32-bit modes in the same order, so the runtime
can answer from the host without its own D3D9 object.

Usage: python display_modes.py [--json out.json]
"""
import ctypes
import json
import sys
from ctypes import POINTER, byref, c_long, c_uint32, c_void_p, wintypes

X8R8G8B8 = 22


class DEVMODEW(ctypes.Structure):
    _fields_ = [
        ("dmDeviceName", wintypes.WCHAR * 32), ("dmSpecVersion", wintypes.WORD),
        ("dmDriverVersion", wintypes.WORD), ("dmSize", wintypes.WORD),
        ("dmDriverExtra", wintypes.WORD), ("dmFields", wintypes.DWORD),
        ("dmPositionX", c_long), ("dmPositionY", c_long),
        ("dmDisplayOrientation", wintypes.DWORD), ("dmDisplayFixedOutput", wintypes.DWORD),
        ("dmColor", ctypes.c_short), ("dmDuplex", ctypes.c_short), ("dmYResolution", ctypes.c_short),
        ("dmTTOption", ctypes.c_short), ("dmCollate", ctypes.c_short),
        ("dmFormName", wintypes.WCHAR * 32), ("dmLogPixels", wintypes.WORD),
        ("dmBitsPerPel", wintypes.DWORD), ("dmPelsWidth", wintypes.DWORD),
        ("dmPelsHeight", wintypes.DWORD), ("dmDisplayFlags", wintypes.DWORD),
        ("dmDisplayFrequency", wintypes.DWORD), ("dmICMMethod", wintypes.DWORD),
        ("dmICMIntent", wintypes.DWORD), ("dmMediaType", wintypes.DWORD),
        ("dmDitherType", wintypes.DWORD), ("dmReserved1", wintypes.DWORD),
        ("dmReserved2", wintypes.DWORD), ("dmPanningWidth", wintypes.DWORD),
        ("dmPanningHeight", wintypes.DWORD),
    ]


def d3d9_modes():
    d3d9 = ctypes.WinDLL("d3d9")
    d3d9.Direct3DCreate9.restype = c_void_p
    d3d = d3d9.Direct3DCreate9(32)
    assert d3d, "Direct3DCreate9 failed"
    vtbl = ctypes.cast(ctypes.cast(d3d, POINTER(c_void_p))[0], POINTER(c_void_p))

    def method(slot, *argtypes):
        f = ctypes.WINFUNCTYPE(c_long, c_void_p, *argtypes)(vtbl[slot])
        return lambda *a: f(d3d, *a) & 0xFFFFFFFF

    U = c_uint32
    count = method(6, U, U)(0, X8R8G8B8)
    enum = method(7, U, U, U, c_void_p)
    display = method(8, U, c_void_p)
    modes = []
    for k in range(count):
        m = (U * 4)()
        assert enum(0, X8R8G8B8, k, m) == 0
        modes.append(tuple(m))
    d = (U * 4)()
    assert display(0, d) == 0
    identifier = ctypes.create_string_buffer(1100)
    method(5, U, U, c_void_p)(0, 0, identifier)
    device = identifier.raw[1024:1056].split(b"\0")[0].decode(errors="replace")
    method(2)()  # Release
    return modes, tuple(d), device


def gdi_modes(device):
    user32 = ctypes.WinDLL("user32")
    modes = []
    k = 0
    while True:
        dm = DEVMODEW()
        dm.dmSize = ctypes.sizeof(DEVMODEW)
        if not user32.EnumDisplaySettingsW(device, k, byref(dm)):
            break
        modes.append((dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency, dm.dmBitsPerPel,
                      dm.dmDisplayFixedOutput, dm.dmDisplayOrientation))
        k += 1
    cur = DEVMODEW()
    cur.dmSize = ctypes.sizeof(DEVMODEW)
    user32.EnumDisplaySettingsW(device, -1, byref(cur))  # ENUM_CURRENT_SETTINGS
    return modes, (cur.dmPelsWidth, cur.dmPelsHeight, cur.dmDisplayFrequency, cur.dmBitsPerPel)


def main():
    d3d, desktop, device = d3d9_modes()
    gdi, current = gdi_modes(device)
    print(f"adapter device {device!r}: D3D9 {len(d3d)} X8R8G8B8 modes, desktop {desktop}")
    print(f"EnumDisplaySettingsW: {len(gdi)} modes, current {current}")
    g32 = [(w, h, hz, X8R8G8B8) for (w, h, hz, bpp, _, orient) in gdi if bpp == 32]
    print("same list as the 32-bit EnumDisplaySettingsW modes, in order:", g32 == d3d)
    first = list(dict.fromkeys(g32))
    print("D3D9's list is the 32-bit EnumDisplaySettingsW modes, each (w, h, Hz) once in first-seen order:", first == d3d)
    print("D3D9's desktop is ENUM_CURRENT_SETTINGS:", desktop == (current[0], current[1], current[2], X8R8G8B8))
    s32 = sorted(set(g32))
    print("same set:", sorted(set(d3d)) == s32, "| D3D9 sorted by (w, h, hz):", d3d == sorted(d3d), "| duplicates in GDI 32-bit list:", len(g32) - len(set(g32)))
    only_d3d = sorted(set(d3d) - set(g32))
    only_gdi = sorted(set(g32) - set(d3d))
    print("only in D3D9:", only_d3d[:20])
    print("only in GDI:", only_gdi[:20])
    for m in d3d:
        print("  d3d9", m)
    if len(sys.argv) > 2 and sys.argv[1] == "--json":
        json.dump({"device": device, "d3d9": d3d, "desktop": desktop, "gdi": gdi, "current": current}, open(sys.argv[2], "w"), indent=1)


if __name__ == "__main__":
    main()
