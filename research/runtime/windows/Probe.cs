using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
public static class Probe {
  [DllImport("kernel32.dll", SetLastError=true)] static extern int MultiByteToWideChar(uint cp, uint flags, byte[] mb, int cb, ushort[] wc, int cch);
  [DllImport("kernel32.dll", SetLastError=true)] static extern int WideCharToMultiByte(uint cp, uint flags, ushort[] wc, int cch, byte[] mb, int cb, byte[] defch, IntPtr used);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetStringTypeW(uint type, ushort[] s, int n, ushort[] outp);
  [DllImport("kernel32.dll", SetLastError=true)] static extern int LCMapStringW(uint lcid, uint flags, ushort[] s, int n, ushort[] d, int dn);
  [DllImport("kernel32.dll", SetLastError=true)] static extern int CompareStringW(uint lcid, uint flags, ushort[] a, int na, ushort[] b, int nb);
  static StreamWriter w;
  static string Hex(byte[] b, int n) { var s = new StringBuilder(); for (int i = 0; i < n && i < b.Length; i++) s.AppendFormat("{0:X2} ", b[i]); return s.ToString().Trim(); }
  static string HexW(ushort[] b, int n) { var s = new StringBuilder(); for (int i = 0; i < n && i < b.Length; i++) s.AppendFormat("{0:X4} ", b[i]); return s.ToString().Trim(); }
  static ushort[] W(string s) { var r = new ushort[s.Length]; for (int i = 0; i < s.Length; i++) r[i] = s[i]; return r; }
  static ushort[] WZ(string s) { var r = new ushort[s.Length+1]; for (int i = 0; i < s.Length; i++) r[i] = s[i]; return r; }
  static void M(string label, uint cp, uint flags, byte[] src, int cb, int cch) {
    ushort[] dst = cch > 0 ? new ushort[cch + 2] : null;
    if (dst != null) for (int i = 0; i < dst.Length; i++) dst[i] = 0xEEEE;
    int r = MultiByteToWideChar(cp, flags, src, cb, dst, cch); int e = Marshal.GetLastWin32Error();
    w.WriteLine("MB {0}: r={1} err={2} out=[{3}]", label, r, r == 0 ? e : 0, dst == null ? "" : HexW(dst, dst.Length));
  }
  static void C(string label, uint cp, uint flags, ushort[] src, int cch, int cb, byte[] defch, bool wantUsed) {
    byte[] dst = cb > 0 ? new byte[cb + 2] : null;
    if (dst != null) for (int i = 0; i < dst.Length; i++) dst[i] = 0xEE;
    IntPtr used = IntPtr.Zero; if (wantUsed) { used = Marshal.AllocHGlobal(4); Marshal.WriteInt32(used, 0x55); }
    int r = WideCharToMultiByte(cp, flags, src, cch, dst, cb, defch, used); int e = Marshal.GetLastWin32Error();
    w.WriteLine("WC {0}: r={1} err={2} used={3} out=[{4}]", label, r, r == 0 ? e : 0, wantUsed ? Marshal.ReadInt32(used).ToString("X") : "-", dst == null ? "" : Hex(dst, dst.Length));
  }
  static void G(string label, uint type, ushort[] s, int n) {
    var o = new ushort[6]; for (int i=0;i<6;i++) o[i]=0xEEEE;
    bool r = GetStringTypeW(type, s, n, o); int e = Marshal.GetLastWin32Error();
    w.WriteLine("GST {0}: r={1} err={2} out=[{3}]", label, r, r?0:e, HexW(o,6));
  }
  static void L(string label, uint lcid, uint flags, ushort[] s, int n, int cd) {
    ushort[] d = cd > 0 ? new ushort[cd+2] : null; if (d != null) for (int i=0;i<d.Length;i++) d[i]=0xEEEE;
    int r = LCMapStringW(lcid, flags, s, n, d, cd); int e = Marshal.GetLastWin32Error();
    w.WriteLine("LCM {0}: r={1} err={2} out=[{3}]", label, r, r==0?e:0, d==null?"":HexW(d,d.Length));
  }
  static void K(string label, uint lcid, uint flags, ushort[] a, int na, ushort[] b, int nb) {
    int r = CompareStringW(lcid, flags, a, na, b, nb); int e = Marshal.GetLastWin32Error();
    w.WriteLine("CMP {0}: r={1} err={2}", label, r, r==0?e:0);
  }
  public static void Run(string path) {
    w = new StreamWriter(path);
    var abc = new byte[]{0x61,0x62,0x63,0};
    M("abc cb3 size", 1252, 0, abc, 3, 0);
    M("abc cb-1 size", 1252, 0, abc, -1, 0);
    M("abc cb-1 cch2", 1252, 0, abc, -1, 2);
    M("abc cb-1 cch4", 1252, 0, abc, -1, 4);
    M("cb0", 1252, 0, abc, 0, 4);
    M("flags3", 1252, 3, abc, 3, 4);
    M("flags0x10", 1252, 0x10, abc, 3, 4);
    M("flags2", 1252, 2, new byte[]{0xE9}, 1, 4);
    M("flags4", 1252, 4, new byte[]{0x01}, 1, 4);
    M("cch-1", 1252, 0, abc, 3, -1);
    M("null src", 1252, 0, null, 3, 4);
    M("0x81 err", 1252, 9, new byte[]{0x81}, 1, 4);
    M("cp0", 0, 1, new byte[]{0x80}, 1, 4);
    M("cp3 thread", 3, 0, new byte[]{0x80}, 1, 4);
    M("cp1 oem", 1, 0, new byte[]{0x80}, 1, 4);
    M("cp437", 437, 0, new byte[]{0x80}, 1, 4);
    M("cp12345", 12345, 0, new byte[]{0x80}, 1, 4);
    M("u8 a80b", 65001, 0, new byte[]{0x61,0x80,0x62}, 3, 8);
    M("u8 a80b err", 65001, 8, new byte[]{0x61,0x80,0x62}, 3, 8);
    M("u8 overlong C080", 65001, 0, new byte[]{0xC0,0x80}, 2, 8);
    M("u8 trunc E282", 65001, 0, new byte[]{0xE2,0x82}, 2, 8);
    M("u8 E282 41", 65001, 0, new byte[]{0xE2,0x82,0x41}, 3, 8);
    M("u8 surrogate EDA080", 65001, 0, new byte[]{0xED,0xA0,0x80}, 3, 8);
    M("u8 F09F9880", 65001, 0, new byte[]{0xF0,0x9F,0x98,0x80}, 4, 8);
    M("u8 F4908080", 65001, 0, new byte[]{0xF4,0x90,0x80,0x80}, 4, 8);
    M("u8 F09F98", 65001, 0, new byte[]{0xF0,0x9F,0x98}, 3, 8);
    M("u8 E0809F", 65001, 0, new byte[]{0xE0,0x80,0x9F}, 3, 8);
    M("u8 FF FE", 65001, 0, new byte[]{0xFF,0xFE,0x41}, 3, 8);
    M("u8 C2", 65001, 0, new byte[]{0x41,0xC2}, 2, 8);
    M("u8 F8888080 80", 65001, 0, new byte[]{0xF8,0x88,0x80,0x80,0x80}, 5, 8);
    M("u8 flags1", 65001, 1, abc, 3, 8);
    M("u8 pair cch1", 65001, 0, new byte[]{0xF0,0x9F,0x98,0x80}, 4, 1);
    M("u8 ab-pair cch2", 65001, 0, new byte[]{0x61,0xF0,0x9F,0x98,0x80}, 5, 2);
    M("u8 size invalid", 65001, 0, new byte[]{0x61,0x80,0x80,0x62}, 4, 0);
    M("u8 size invalid err", 65001, 8, new byte[]{0x61,0x80,0x62}, 3, 0);
    M("1252 bom", 1252, 0, new byte[]{0xEF,0xBB,0xBF}, 3, 4);
    var wabc = WZ("abc");
    C("abc -1 size", 1252, 0, wabc, -1, 0, null, false);
    C("abc 3 cb2", 1252, 0, wabc, 3, 2, null, true);
    C("abc -1 cb3", 1252, 0, wabc, -1, 3, null, true);
    C("a A-macron euro", 1252, 0, W("aĀ€"), 3, 4, null, true);
    C("a cjk", 1252, 0, W("a一"), 2, 4, null, true);
    C("a cjk star", 1252, 0, W("a一"), 2, 4, new byte[]{0x2A,0}, true);
    C("pair", 1252, 0, W("😀"), 2, 4, null, true);
    C("lone high a", 1252, 0, W("\uD83Da"), 2, 4, null, true);
    C("lone low", 1252, 0, W("\uDE00"), 1, 4, null, true);
    C("nobf macron", 1252, 0x400, W("Ā"), 1, 4, null, true);
    C("flags 0x80 1252", 1252, 0x80, wabc, 3, 4, null, true);
    C("flags 0x200", 1252, 0x200, W("é"), 2, 4, null, true);
    C("flags 0x40", 1252, 0x40, W("é"), 2, 4, null, true);
    C("flags 0x1000", 1252, 0x1000, wabc, 3, 4, null, true);
    C("cch0", 1252, 0, wabc, 0, 4, null, true);
    C("cb-1", 1252, 0, wabc, 3, -1, null, true);
    C("null src", 1252, 0, null, 3, 4, null, true);
    C("u8 a euro", 65001, 0, W("a€"), 2, 8, null, false);
    C("u8 lone D800", 65001, 0, W("\uD800a"), 2, 8, null, false);
    C("u8 lone D800 err", 65001, 0x80, W("\uD800a"), 2, 8, null, false);
    C("u8 used ptr", 65001, 0, wabc, 3, 8, null, true);
    C("u8 defch", 65001, 0, wabc, 3, 8, new byte[]{0x2A,0}, false);
    C("u8 pair", 65001, 0, W("😀"), 2, 8, null, false);
    C("u8 euro cb2", 65001, 0, W("a€"), 2, 2, null, false);
    C("u8 flags 0x400", 65001, 0x400, wabc, 3, 8, null, false);
    C("u8 low then high", 65001, 0, W("\uDE00\uD83D"), 2, 8, null, false);
    G("-1", 1, WZ("aB1"), -1);
    G("type3", 3, WZ("aB1"), 3);
    G("cch0", 1, WZ("aB1"), 0);
    G("cch-2", 1, WZ("aB1"), -2);
    G("null", 1, null, 3);
    G("embedded nul", 1, W("a\0b"), 3);
    L("upper -1 size", 0x409, 0x200, WZ("Abc"), -1, 0);
    L("upper -1 cch4", 0x409, 0x200, WZ("Abc"), -1, 4);
    L("upper -1 cch2", 0x409, 0x200, WZ("Abc"), -1, 2);
    L("upper 3 cch3", 0x409, 0x200, WZ("Abc"), 3, 3);
    L("upper|lower", 0x409, 0x300, WZ("Abc"), 3, 4);
    L("flags0", 0x409, 0, WZ("Abc"), 3, 4);
    L("cch0", 0x409, 0x200, WZ("Abc"), 0, 4);
    L("lcid 0x7f", 0x7F, 0x200, WZ("Abc"), 3, 4);
    L("lcid 0x12345", 0x12345, 0x200, WZ("Abc"), 3, 4);
    L("lcid 0x400", 0x400, 0x200, WZ("ÿ"), 1, 4);
    L("lcid 0x800", 0x800, 0x200, WZ("ÿ"), 1, 4);
    L("cchdest -1", 0x409, 0x200, WZ("Abc"), 3, -1);
    L("embedded nul", 0x409, 0x200, W("a\0b"), 3, 4);
    L("sortkey size", 0x409, 0x400, WZ("ab"), -1, 0);
    L("sortkey small", 0x409, 0x400, WZ("ab"), -1, 2);
    L("sortkey|lower", 0x409, 0x500, WZ("ab"), -1, 6);
    L("sortkey|stringsort", 0x409, 0x1400, WZ("a-b"), -1, 8);
    L("upper|stringsort", 0x409, 0x1200, WZ("ab"), -1, 4);
    L("upper|ignorecase", 0x409, 0x201, WZ("ab"), -1, 4);
    L("lower|0x1000000 linguistic", 0x409, 0x01000100, WZ("AB"), -1, 4);
    K("abc ABC 1001", 0x409, 0x1001, WZ("abc"), -1, WZ("ABC"), -1);
    K("abc ABC 0", 0x409, 0, WZ("abc"), -1, WZ("ABC"), -1);
    K("bad flags", 0x409, 0x80000000, WZ("a"), -1, WZ("a"), -1);
    K("null", 0x409, 0, null, 1, WZ("a"), -1);
    K("cch -2", 0x409, 0, WZ("a"), -2, WZ("a"), -1);
    K("a vs a nul b", 0x409, 0x1000, WZ("a"), -1, W("a\0b"), 3);
    K("a vs a nul", 0x409, 0x1000, WZ("a"), -1, W("a\0"), 2);
    K("empty", 0x409, 0, WZ(""), 0, WZ(""), -1);
    K("co-op coop word", 0x409, 0, WZ("co-op"), -1, WZ("coop"), -1);
    K("co-op coop string", 0x409, 0x1000, WZ("co-op"), -1, WZ("coop"), -1);
    K("lcid 0x7f", 0x7F, 0, WZ("a"), -1, WZ("b"), -1);
    K("lcid 0x12345", 0x12345, 0, WZ("a"), -1, WZ("b"), -1);
    K("lcid 0", 0, 0, WZ("a"), -1, WZ("b"), -1);
    K("ignorenonspace", 0x409, 0x2, WZ("é"), -1, WZ("e"), -1);
    w.Close();
  }
}
