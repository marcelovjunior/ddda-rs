using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
public static class Reg {
  [DllImport("advapi32.dll", CharSet=CharSet.Ansi)] static extern int RegOpenKeyExA(UIntPtr key, string sub, uint opt, uint sam, ref UIntPtr result);
  [DllImport("advapi32.dll", CharSet=CharSet.Ansi)] static extern int RegQueryValueExA(UIntPtr key, string name, IntPtr reserved, IntPtr type, byte[] data, IntPtr cb);
  [DllImport("advapi32.dll")] static extern int RegCloseKey(UIntPtr key);
  static StreamWriter w;
  static UIntPtr HKLM = (UIntPtr)0x80000002u;
  const string CPU = "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0";
  static void O(string label, UIntPtr k, string sub, uint opt, uint sam) {
    UIntPtr r = (UIntPtr)0x55u; int e = RegOpenKeyExA(k, sub, opt, sam, ref r);
    w.WriteLine("OPEN {0}: {1} result={2:X}", label, e, (ulong)r); if (e == 0) RegCloseKey(r);
  }
  static void Q(string label, UIntPtr k, string name, bool wantType, int dataLen, int cbIn, bool cbNull) {
    IntPtr type = IntPtr.Zero; if (wantType) { type = Marshal.AllocHGlobal(4); Marshal.WriteInt32(type, 0x55); }
    IntPtr cb = IntPtr.Zero; if (!cbNull) { cb = Marshal.AllocHGlobal(4); Marshal.WriteInt32(cb, cbIn); }
    byte[] data = dataLen < 0 ? null : new byte[dataLen + 2]; if (data != null) for (int i = 0; i < data.Length; i++) data[i] = 0xEE;
    int e = RegQueryValueExA(k, name, IntPtr.Zero, type, data, cb);
    w.WriteLine("QUERY {0}: {1} type={2} cb={3} data={4}", label, e, wantType ? Marshal.ReadInt32(type).ToString() : "-", cbNull ? "-" : Marshal.ReadInt32(cb).ToString(), data == null ? "null" : BitConverter.ToString(data));
  }
  public static void Run(string path) {
    w = new StreamWriter(path);
    O("cpu read", HKLM, CPU, 0, 0x20019);
    O("cpu lower case", HKLM, CPU.ToLower(), 0, 0x20019);
    O("nope", HKLM, "HARDWARE\\Nope", 0, 0x20019);
    O("cpu write", HKLM, CPU, 0, 0x20006);
    O("cpu all", HKLM, CPU, 0, 0xF003F);
    O("cpu set value", HKLM, CPU, 0, 0x2);
    O("leading backslash", HKLM, "\\" + CPU, 0, 0x20019);
    O("trailing backslash", HKLM, CPU + "\\", 0, 0x20019);
    O("double backslash", HKLM, "HARDWARE\\\\DESCRIPTION", 0, 0x20019);
    O("null sub", HKLM, null, 0, 0x20019);
    O("empty sub", HKLM, "", 0, 0x20019);
    O("bad key", (UIntPtr)0x1234u, "x", 0, 0x20019);
    O("options 1", HKLM, CPU, 1, 0x20019);
    O("options 8", HKLM, CPU, 8, 0x20019);
    O("intermediate", HKLM, "HARDWARE\\DESCRIPTION", 0, 0x20019);
    UIntPtr k = UIntPtr.Zero; RegOpenKeyExA(HKLM, CPU, 0, 0x20019, ref k);
    Q("mhz", k, "~MHz", true, 4, 4, false);
    Q("mhz small", k, "~MHz", true, 2, 2, false);
    Q("mhz size", k, "~MHz", true, -1, 0, false);
    Q("mhz cb null data null", k, "~MHz", true, -1, 0, true);
    Q("mhz cb null", k, "~MHz", true, 4, 0, true);
    Q("name", k, "ProcessorNameString", true, 0xff, 0xff, false);
    Q("name small", k, "ProcessorNameString", true, 10, 10, false);
    Q("name exact no nul", k, "ProcessorNameString", true, 48, 48, false);
    Q("name case", k, "processornamestring", true, 0xff, 0xff, false);
    Q("missing", k, "Nope", true, 4, 4, false);
    Q("default value", k, null, true, 4, 4, false);
    Q("bad handle", (UIntPtr)0x1234u, "~MHz", true, 4, 4, false);
    w.WriteLine("CLOSE: {0}", RegCloseKey(k));
    w.WriteLine("CLOSE again: {0}", RegCloseKey(k));
    w.WriteLine("CLOSE HKLM: {0}", RegCloseKey(HKLM));
    w.WriteLine("CLOSE bad: {0}", RegCloseKey((UIntPtr)0x1234u));
    w.Close();
  }
}
