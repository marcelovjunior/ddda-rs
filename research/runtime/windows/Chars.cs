using System;
using System.IO;
using System.Runtime.InteropServices;
public static class Chars {
  [DllImport("kernel32.dll", SetLastError=true)] public static extern int MultiByteToWideChar(uint cp, uint flags, byte[] mb, int cb, ushort[] wc, int cch);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern bool GetStringTypeW(uint type, ushort[] s, int n, ushort[] outp);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern int LCMapStringW(uint lcid, uint flags, ushort[] s, int n, ushort[] d, int dn);
  [DllImport("kernel32.dll", SetLastError=true, EntryPoint="LCMapStringW")] public static extern int LCMapStringWB(uint lcid, uint flags, ushort[] s, int n, byte[] d, int dn);
  static string Key(uint lcid, uint flags, ushort[] s) {
    var b = new byte[256]; int n = LCMapStringWB(lcid, flags | 0x400, s, s.Length, b, 256);
    var sb = new System.Text.StringBuilder(); for (int i = 0; i < n; i++) sb.AppendFormat("{0:X2}", b[i]); return sb.ToString();
  }
  public static void Run(string path) {
    var sw = new StreamWriter(path);
    for (int b = 0; b < 256; b++) {
      var o = new ushort[2]; MultiByteToWideChar(1252, 0, new byte[]{(byte)b}, 1, o, 2);
      var s = new ushort[]{o[0]};
      var t1 = new ushort[1]; var t2 = new ushort[1]; var t3 = new ushort[1];
      GetStringTypeW(1, s, 1, t1); GetStringTypeW(2, s, 1, t2); GetStringTypeW(4, s, 1, t3);
      var lo = new ushort[4]; var up = new ushort[4]; var lo0 = new ushort[4]; var up0 = new ushort[4];
      int nl = LCMapStringW(0x409, 0x100, s, 1, lo, 4); int nu = LCMapStringW(0x409, 0x200, s, 1, up, 4);
      LCMapStringW(0, 0x100, s, 1, lo0, 4); LCMapStringW(0, 0x200, s, 1, up0, 4);
      sw.WriteLine("{0:X4} c1={1:X4} c2={2:X4} c3={3:X4} lo={4}:{5:X4} up={6}:{7:X4} lo0={8:X4} up0={9:X4} k={10} ks={11} ki={12}",
        o[0], t1[0], t2[0], t3[0], nl, lo[0], nu, up[0], lo0[0], up0[0], Key(0x409,0,s), Key(0x409,0x1000,s), Key(0x409,0x1001,s));
    }
    sw.Close();
  }
}
