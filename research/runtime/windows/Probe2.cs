using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
public static class Probe2 {
  [DllImport("kernel32.dll", SetLastError=true)] static extern int MultiByteToWideChar(uint cp, uint flags, byte[] mb, int cb, ushort[] wc, int cch);
  [DllImport("kernel32.dll", SetLastError=true)] static extern int WideCharToMultiByte(uint cp, uint flags, ushort[] wc, int cch, byte[] mb, int cb, byte[] defch, IntPtr used);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GlobalAlloc(uint f, UIntPtr n);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GlobalLock(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GlobalUnlock(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GlobalFree(IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr LocalAlloc(uint f, UIntPtr n);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Ansi)] static extern bool SetEnvironmentVariableA(string n, string v);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Ansi)] static extern uint GetEnvironmentVariableA(string n, StringBuilder b, uint sz);
  [DllImport("kernel32.dll", SetLastError=true)] static extern ulong VerSetConditionMask(ulong m, uint t, byte c);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool FileTimeToSystemTime(ref long ft, ushort[] st);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool FileTimeToLocalFileTime(ref long ft, out long lt);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetStdHandle(int n, IntPtr h);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GetStdHandle(int n);
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  static StreamWriter w;
  static string Hex(byte[] b, int n) { var s = new StringBuilder(); for (int i = 0; i < n && i < b.Length; i++) s.AppendFormat("{0:X2} ", b[i]); return s.ToString().Trim(); }
  static string HexW(ushort[] b, int n) { var s = new StringBuilder(); for (int i = 0; i < n && i < b.Length; i++) s.AppendFormat("{0:X4} ", b[i]); return s.ToString().Trim(); }
  static ushort[] W(string s) { var r = new ushort[s.Length]; for (int i = 0; i < s.Length; i++) r[i] = s[i]; return r; }
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
  public static void Run(string path) {
    w = new StreamWriter(path);
    var abc = new byte[]{0x61,0x62,0x63};
    foreach (uint f in new uint[]{2, 4, 0x10, 0x80, 3, 9}) M("u8 flags " + f, 65001, f, abc, 3, 4);
    M("u8 err cch1 invalid", 65001, 8, new byte[]{0x61,0x80,0x62}, 3, 1);
    M("u8 noerr cch1 invalid", 65001, 0, new byte[]{0x61,0x80,0x62}, 3, 1);
    M("1252 same buffer?", 1252, 0, abc, 3, 0);
    foreach (uint f in new uint[]{0x200, 0x40, 0x10, 0x1000, 0x8, 0x1, 0x420}) C("u8 flags " + f, 65001, f, W("ab"), 2, 4, null, false);
    C("1252 size used", 1252, 0, W("a一"), 2, 0, null, true);
    C("1252 size used ok", 1252, 0, W("ab"), 2, 0, null, true);
    C("1252 0x10 nocc", 1252, 0x10, W("ab"), 2, 4, null, true);
    C("1252 0x210", 1252, 0x210, W("ab"), 2, 4, null, true);
    C("u8 err size invalid", 65001, 0x80, W("\uD800"), 1, 0, null, false);
    C("u8 err small invalid", 65001, 0x80, W("a\uD800"), 2, 1, null, false);
    // Global/Local
    IntPtr h = GlobalAlloc(0x42, (UIntPtr)10);
    IntPtr p = GlobalLock(h); IntPtr p2 = GlobalLock(h);
    w.WriteLine("GA moveable h={0:X} p={1:X} p2={2:X} same={3}", (long)h, (long)p, (long)p2, h == p);
    bool u1 = GlobalUnlock(h); int e1 = Marshal.GetLastWin32Error();
    bool u2 = GlobalUnlock(h); int e2 = Marshal.GetLastWin32Error();
    SetLastError(0);
    bool u3 = GlobalUnlock(h); int e3 = Marshal.GetLastWin32Error();
    w.WriteLine("GU moveable: {0} {1} / {2} {3} / {4} {5}", u1, e1, u2, e2, u3, e3);
    IntPtr f1 = GlobalFree(h); int ef = Marshal.GetLastWin32Error();
    w.WriteLine("GF moveable: {0:X} {1}", (long)f1, ef);
    IntPtr hf = GlobalAlloc(0x40, (UIntPtr)10); IntPtr pf = GlobalLock(hf);
    SetLastError(0);
    bool uf = GlobalUnlock(hf); int euf = Marshal.GetLastWin32Error();
    w.WriteLine("GA fixed h={0:X} lock={1:X} unlock={2} err={3}", (long)hf, (long)pf, uf, euf);
    w.WriteLine("GF fixed: {0:X}", (long)GlobalFree(hf));
    SetLastError(0);
    w.WriteLine("GF null: {0:X} {1}", (long)GlobalFree(IntPtr.Zero), Marshal.GetLastWin32Error());
    IntPtr h0 = GlobalAlloc(2, (UIntPtr)0); SetLastError(0); IntPtr p0 = GlobalLock(h0); int e0 = Marshal.GetLastWin32Error();
    w.WriteLine("GA moveable 0: h={0:X} lock={1:X} err={2}", (long)h0, (long)p0, e0);
    IntPtr hz = GlobalAlloc(0, (UIntPtr)0); w.WriteLine("GA fixed 0: {0:X} err={1}", (long)hz, Marshal.GetLastWin32Error());
    SetLastError(0); IntPtr hb = GlobalAlloc(0x1000000, (UIntPtr)8); w.WriteLine("GA bad flags: {0:X} err={1}", (long)hb, Marshal.GetLastWin32Error());
    SetLastError(0); IntPtr hbig = GlobalAlloc(0, (UIntPtr)0xFFFFFFF0); w.WriteLine("GA huge: {0:X} err={1}", (long)hbig, Marshal.GetLastWin32Error());
    IntPtr la = LocalAlloc(0x40, (UIntPtr)16); w.WriteLine("LA fixed {0:X} zero={1}", (long)la, Marshal.ReadInt64(la) == 0);
    IntPtr lm = LocalAlloc(0x42, (UIntPtr)16); w.WriteLine("LA moveable {0:X}", (long)lm);
    // environment
    SetLastError(0); bool s1 = SetEnvironmentVariableA("DDRT_X", "1"); w.WriteLine("SEV set: {0} {1}", s1, Marshal.GetLastWin32Error());
    SetLastError(0); bool s2 = SetEnvironmentVariableA("DDRT_X", null); w.WriteLine("SEV del: {0} {1}", s2, Marshal.GetLastWin32Error());
    SetLastError(0); bool s3 = SetEnvironmentVariableA("DDRT_NOPE", null); w.WriteLine("SEV del missing: {0} {1}", s3, Marshal.GetLastWin32Error());
    SetLastError(0); bool s4 = SetEnvironmentVariableA("A=B", "1"); w.WriteLine("SEV name with =: {0} {1}", s4, Marshal.GetLastWin32Error());
    SetLastError(0); bool s5 = SetEnvironmentVariableA("", "1"); w.WriteLine("SEV empty name: {0} {1}", s5, Marshal.GetLastWin32Error());
    SetLastError(0); bool s6 = SetEnvironmentVariableA("=X", "1"); w.WriteLine("SEV =X: {0} {1}", s6, Marshal.GetLastWin32Error());
    SetLastError(0); bool s7 = SetEnvironmentVariableA("DDRT_E", ""); var sb = new StringBuilder(8); uint g7 = GetEnvironmentVariableA("DDRT_E", sb, 8); w.WriteLine("SEV empty value: {0} {1} get={2} err={3}", s7, 0, g7, Marshal.GetLastWin32Error());
    SetLastError(0); bool s8 = SetEnvironmentVariableA(null, "1"); w.WriteLine("SEV null name: {0} {1}", s8, Marshal.GetLastWin32Error());
    // version
    foreach (var t in new uint[][]{ new uint[]{3,3}, new uint[]{2,3}, new uint[]{1,3}, new uint[]{0,3}, new uint[]{3,0}, new uint[]{3,8}, new uint[]{0x80,1}, new uint[]{0x40,6}, new uint[]{0x20,7}, new uint[]{0xC0,1}})
      w.WriteLine("VSCM({0:X},{1}) = {2:X}", t[0], t[1], VerSetConditionMask(0, t[0], (byte)t[1]));
    w.WriteLine("VSCM chained = {0:X}", VerSetConditionMask(VerSetConditionMask(0, 2, 3), 1, 3));
    w.WriteLine("VSCM over = {0:X}", VerSetConditionMask(VerSetConditionMask(0, 2, 3), 2, 1));
    // time
    foreach (long ft in new long[]{0, 1, 116444736000000000, 0x7FFFFFFFFFFFFFFF, unchecked((long)0x8000000000000000), 0x0123456789ABCDEF, 130000000000000000, 25000000000000000}) {
      long v = ft; var st = new ushort[8]; for (int i=0;i<8;i++) st[i]=0xEEEE;
      SetLastError(0); bool r = FileTimeToSystemTime(ref v, st); int e = Marshal.GetLastWin32Error();
      w.WriteLine("FT2ST {0:X16}: {1} {2} [{3}]", ft, r, e, HexW(st, 8));
    }
    { long v = unchecked((long)0x8000000000000000); long lt; bool r = FileTimeToLocalFileTime(ref v, out lt); w.WriteLine("FT2LFT big: {0} {1} {2:X}", r, Marshal.GetLastWin32Error(), lt); }
    // std handles
    IntPtr old = GetStdHandle(-12);
    SetLastError(0); bool sh = SetStdHandle(5, (IntPtr)0x1234); w.WriteLine("SSH 5: {0} {1}", sh, Marshal.GetLastWin32Error());
    SetLastError(0); bool sh2 = SetStdHandle(-12, (IntPtr)0x1234); w.WriteLine("SSH -12: {0} {1} get={2:X}", sh2, Marshal.GetLastWin32Error(), (long)GetStdHandle(-12));
    SetStdHandle(-12, old);
    SetLastError(0); IntPtr gb = GetStdHandle(5); w.WriteLine("GSH 5: {0:X} {1}", (long)gb, Marshal.GetLastWin32Error());
    w.Close();
  }
}
