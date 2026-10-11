using System;
using System.IO;
using System.Runtime.InteropServices;
public static class Unmanifested {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern ulong VerSetConditionMask(ulong m, uint t, byte c);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool VerifyVersionInfoA(byte[] info, uint type, ulong mask);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetVersionExA(byte[] info);
  static StreamWriter w;
  static byte[] Ver(uint major, uint minor, uint build, uint plat, ushort spmaj, ushort spmin, ushort suite, byte product) {
    var b = new byte[156];
    BitConverter.GetBytes(156u).CopyTo(b, 0); BitConverter.GetBytes(major).CopyTo(b, 4); BitConverter.GetBytes(minor).CopyTo(b, 8);
    BitConverter.GetBytes(build).CopyTo(b, 12); BitConverter.GetBytes(plat).CopyTo(b, 16);
    BitConverter.GetBytes(spmaj).CopyTo(b, 148); BitConverter.GetBytes(spmin).CopyTo(b, 150); BitConverter.GetBytes(suite).CopyTo(b, 152); b[154] = product;
    return b;
  }
  static void V(string label, byte[] info, uint type, ulong mask) {
    SetLastError(0); bool r = VerifyVersionInfoA(info, type, mask); w.WriteLine("VVI {0}: {1} err={2}", label, r, Marshal.GetLastWin32Error());
  }
  static ulong M(uint t, byte c) { return VerSetConditionMask(0, t, c); }
  public static void Main(string[] args) {
    w = new StreamWriter(args[0]);
    var g = new byte[156]; BitConverter.GetBytes(156u).CopyTo(g, 0); GetVersionExA(g);
    w.WriteLine("GetVersionEx: {0}.{1}.{2} plat {3} sp {4}.{5} suite {6:X} product {7}", BitConverter.ToUInt32(g, 4), BitConverter.ToUInt32(g, 8), BitConverter.ToUInt32(g, 12), BitConverter.ToUInt32(g, 16), BitConverter.ToUInt16(g, 148), BitConverter.ToUInt16(g, 150), BitConverter.ToUInt16(g, 152), g[154]);
    V("game: 5.1 type3 mask(3,3)", Ver(5,1,0,0,0,0,0,0), 3, M(3,3));
    V("6.2 eq", Ver(6,2,0,0,0,0,0,0), 3, VerSetConditionMask(M(2,1), 1, 1));
    V("6.3 ge", Ver(6,3,0,0,0,0,0,0), 3, M(3,3));
    V("10.0 ge", Ver(10,0,0,0,0,0,0,0), 3, M(3,3));
    V("build ge 9200", Ver(0,0,9200,0,0,0,0,0), 4, M(4,3));
    V("build ge 9201", Ver(0,0,9201,0,0,0,0,0), 4, M(4,3));
    V("build eq 26200", Ver(0,0,26200,0,0,0,0,0), 4, M(4,1));
    V("build gt 9200", Ver(0,0,9200,0,0,0,0,0), 4, M(4,2));
    V("build lt 9200", Ver(0,0,9200,0,0,0,0,0), 4, M(4,4));
    V("platform eq 2", Ver(0,0,0,2,0,0,0,0), 8, M(8,1));
    V("platform gt 2", Ver(0,0,0,2,0,0,0,0), 8, M(8,2));
    V("product eq 1", Ver(0,0,0,0,0,0,0,1), 0x80, M(0x80,1));
    V("product eq 3", Ver(0,0,0,0,0,0,0,3), 0x80, M(0x80,1));
    V("sp eq 0", Ver(0,0,0,0,0,0,0,0), 0x30, VerSetConditionMask(M(0x20,1), 0x10, 1));
    V("spmajor ge 1", Ver(0,0,0,0,1,0,0,0), 0x20, M(0x20,3));
    V("suite and 0x100", Ver(0,0,0,0,0,0,0x100,0), 0x40, M(0x40,6));
    V("suite or 0x300", Ver(0,0,0,0,0,0,0x300,0), 0x40, M(0x40,7));
    V("suite and 0", Ver(0,0,0,0,0,0,0,0), 0x40, M(0x40,6));
    V("build cond0", Ver(0,0,9200,0,0,0,0,0), 4, 0x8000000000000000);
    V("build cond0 diff", Ver(0,0,9201,0,0,0,0,0), 4, 0x8000000000000000);
    w.Close();
  }
}
