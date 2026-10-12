using System;
using System.IO;
using System.Runtime.InteropServices;
// GetCPInfo for the code pages the CRT can ask about (L21): what Windows
// writes into CPINFO (MaxCharSize, DefaultChar[2], LeadByte[12]) and the
// error for unknown pages.
public static class CpInfo {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetCPInfo(uint cp, byte[] info);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool IsValidCodePage(uint cp);
  public static void Run(string path) {
    var w = new StreamWriter(path);
    foreach (uint cp in new uint[]{0, 1, 2, 3, 42, 437, 850, 932, 1252, 65000, 65001, 12345, 0xFFFF}) {
      var b = new byte[20]; for (int i = 0; i < 20; i++) b[i] = 0xEE;
      SetLastError(0); bool r = GetCPInfo(cp, b); int e = Marshal.GetLastWin32Error();
      SetLastError(0); bool v = IsValidCodePage(cp); int ev = Marshal.GetLastWin32Error();
      w.WriteLine("cp {0}: GetCPInfo={1} err={2} [{3}] IsValidCodePage={4} err={5}", cp, r, e, BitConverter.ToString(b), v, ev);
    }
    w.Close();
  }
}
