using System;
using System.IO;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class Vectors {
  [DllImport("kernel32.dll", SetLastError=true)] static extern int MultiByteToWideChar(uint cp, uint flags, byte[] mb, int cb, ushort[] wc, int cch);
  [DllImport("kernel32.dll", SetLastError=true)] static extern int WideCharToMultiByte(uint cp, uint flags, ushort[] wc, int cch, byte[] mb, int cb, byte[] defch, IntPtr used);
  [DllImport("kernel32.dll", SetLastError=true, EntryPoint="LCMapStringW")] static extern int LCMapStringWB(uint lcid, uint flags, ushort[] s, int n, byte[] d, int dn);
  [DllImport("kernel32.dll", SetLastError=true)] static extern int CompareStringW(uint lcid, uint flags, ushort[] a, int na, ushort[] b, int nb);
  static string Hex(byte[] b, int n) { var s = new StringBuilder(); for (int i = 0; i < n; i++) s.AppendFormat("{0:X2}", b[i]); return s.ToString(); }
  static string HexW(ushort[] b, int n) { var s = new StringBuilder(); for (int i = 0; i < n; i++) s.AppendFormat("{0:X4}", b[i]); return s.ToString(); }
  public static void Run(string dir) {
    var rng = new Random(1234);
    // UTF-8 decode: structured random bytes
    byte[] pool = {0x00,0x41,0x7F,0x80,0x8F,0x90,0x9F,0xA0,0xBF,0xC0,0xC1,0xC2,0xDF,0xE0,0xE1,0xEC,0xED,0xEE,0xEF,0xF0,0xF1,0xF3,0xF4,0xF5,0xF7,0xF8,0xFB,0xFC,0xFE,0xFF};
    using (var w = new StreamWriter(Path.Combine(dir, "utf8_decode.txt"))) {
      for (int c = 0; c < 6000; c++) {
        int n = 1 + rng.Next(7); var b = new byte[n];
        for (int i = 0; i < n; i++) b[i] = rng.Next(3) == 0 ? (byte)rng.Next(256) : pool[rng.Next(pool.Length)];
        foreach (uint fl in new uint[]{0, 8}) {
          var o = new ushort[32]; int r = MultiByteToWideChar(65001, fl, b, n, o, 32); int e = Marshal.GetLastWin32Error();
          w.WriteLine("{0} {1} {2} {3} {4}", Hex(b, n), fl, r, r == 0 ? e : 0, HexW(o, r == 0 ? 0 : r));
        }
      }
    }
    // UTF-8 / 1252 encode: random UTF-16 with surrogates and best-fit candidates
    ushort[] wpool = {0x41,0x7F,0x80,0x9F,0xA0,0xFF,0x100,0x141,0x2C6,0x391,0x4E00,0x20AC,0x2122,0xD800,0xDBFF,0xDC00,0xDFFF,0xD83D,0xDE00,0xFFFD,0xFFFF,0xFF21,0x2013};
    using (var w = new StreamWriter(Path.Combine(dir, "wc_encode.txt"))) {
      for (int c = 0; c < 4000; c++) {
        int n = 1 + rng.Next(5); var s = new ushort[n];
        for (int i = 0; i < n; i++) s[i] = rng.Next(3) == 0 ? (ushort)rng.Next(0x10000) : wpool[rng.Next(wpool.Length)];
        foreach (uint cp in new uint[]{65001, 1252}) foreach (uint fl in (cp == 65001 ? new uint[]{0, 0x80} : new uint[]{0, 0x400})) {
          var o = new byte[64]; IntPtr used = IntPtr.Zero;
          if (cp == 1252) { used = Marshal.AllocHGlobal(4); Marshal.WriteInt32(used, 0x55); }
          int r = WideCharToMultiByte(cp, fl, s, n, o, 64, null, used); int e = Marshal.GetLastWin32Error();
          w.WriteLine("{0} {1} {2} {3} {4} {5}", cp, HexW(s, n), fl, r, r == 0 ? e : 0, Hex(o, r == 0 ? 0 : r) + (cp == 1252 ? " " + Marshal.ReadInt32(used) : ""));
          if (used != IntPtr.Zero) Marshal.FreeHGlobal(used);
        }
      }
    }
    // sort keys and comparisons over the cp1252 repertoire
    var rep = new ushort[256];
    for (int b = 0; b < 256; b++) { var o = new ushort[2]; MultiByteToWideChar(1252, 0, new byte[]{(byte)b}, 1, o, 2); rep[b] = o[0]; }
    Func<ushort[]> rs = () => { int n = rng.Next(7); var s = new ushort[n]; for (int i = 0; i < n; i++) s[i] = rng.Next(2) == 0 ? rep[rng.Next(256)] : (ushort)"aAbB-'eEéÉæÆß ss0"[rng.Next(17)]; return s; };
    using (var w = new StreamWriter(Path.Combine(dir, "sortkeys.txt"))) {
      for (int c = 0; c < 4000; c++) {
        var s = rs();
        foreach (uint fl in new uint[]{0, 0x1000, 0x1001, 0x1, 0x2, 0x1002, 0x1003}) {
          var k = new byte[512]; int r = LCMapStringWB(0x409, 0x400 | fl, s, s.Length, k, 512);
          w.WriteLine("K {0} {1} {2}", fl, s.Length == 0 ? "-" : HexW(s, s.Length), r == 0 ? "ERR" + Marshal.GetLastWin32Error() : Hex(k, r));
        }
        var a = rs(); var bb = rs();
        if (rng.Next(2) == 0 && a.Length > 0) { bb = (ushort[])a.Clone(); bb[rng.Next(bb.Length)] = rep[rng.Next(256)]; }
        foreach (uint fl in new uint[]{0, 0x1000, 0x1001}) {
          int r = CompareStringW(0x409, fl, a, a.Length, bb, bb.Length);
          w.WriteLine("C {0} {1} {2} {3}", fl, a.Length == 0 ? "-" : HexW(a, a.Length), bb.Length == 0 ? "-" : HexW(bb, bb.Length), r == 0 ? "ERR" + Marshal.GetLastWin32Error() : r.ToString());
        }
      }
    }
  }
}
