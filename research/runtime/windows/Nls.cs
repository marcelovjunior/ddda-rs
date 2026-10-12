using System;
using System.Text;
using System.Runtime.InteropServices;
using System.IO;
public static class Nls {
  [DllImport("kernel32.dll", SetLastError=true)] public static extern int MultiByteToWideChar(uint cp, uint flags, byte[] mb, int cb, ushort[] wc, int cch);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern int WideCharToMultiByte(uint cp, uint flags, ushort[] wc, int cch, byte[] mb, int cb, byte[] defch, out int used);
  [DllImport("kernel32.dll", SetLastError=true, EntryPoint="WideCharToMultiByte")] public static extern int WideCharToMultiByteNoUsed(uint cp, uint flags, ushort[] wc, int cch, byte[] mb, int cb, IntPtr defch, IntPtr used);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern bool GetStringTypeW(uint type, ushort[] s, int n, ushort[] outp);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern int LCMapStringW(uint lcid, uint flags, ushort[] s, int n, ushort[] d, int dn);
  [DllImport("kernel32.dll", SetLastError=true, EntryPoint="LCMapStringW")] public static extern int LCMapStringWB(uint lcid, uint flags, ushort[] s, int n, byte[] d, int dn);
  [DllImport("kernel32.dll", SetLastError=true)] public static extern int CompareStringW(uint lcid, uint flags, ushort[] a, int na, ushort[] b, int nb);

  public static void BestFit(string path) {
    var sw = new StreamWriter(path);
    var dec = new ushort[256];
    for (int b = 0; b < 256; b++) { var o = new ushort[2]; MultiByteToWideChar(1252, 0, new byte[]{(byte)b}, 1, o, 2); dec[b] = o[0]; 
      int e = MultiByteToWideChar(1252, 8, new byte[]{(byte)b}, 1, o, 2); int err = e==0 ? Marshal.GetLastWin32Error() : 0;
      sw.WriteLine("D {0:X2} {1:X4} {2}", b, (int)dec[b], err); }
    int n = 0;
    for (int u = 0; u < 0x10000; u++) {
      if (u >= 0xD800 && u < 0xE000) continue;
      var mb = new byte[4]; int used;
      int r = WideCharToMultiByte(1252, 0, new ushort[]{(ushort)u}, 1, mb, 4, null, out used);
      int used2; var mb2 = new byte[4];
      int r2 = WideCharToMultiByte(1252, 0x400, new ushort[]{(ushort)u}, 1, mb2, 4, null, out used2);
      bool round = dec[mb[0]] == (ushort)u;
      if (!round) { sw.WriteLine("B {0:X4} {1} {2:X2} {3} | nobf {4} {5:X2} {6}", u, r, mb[0], used, r2, mb2[0], used2); if (used==0) n++; }
    }
    sw.WriteLine("bestfit entries {0}", n);
    sw.Close();
  }
}
