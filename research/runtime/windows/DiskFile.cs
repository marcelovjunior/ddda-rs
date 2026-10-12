using System;
using System.IO;
using System.Runtime.InteropServices;
// GetDiskFreeSpaceA with the paths MtFile::vf05 passes in its overlapped
// open mode (0x12): the full path of the file being opened, built by
// 0x00D4EB30 (L21). Read-only queries.
public static class DiskFile {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Ansi)] static extern bool GetDiskFreeSpaceA(string root, IntPtr spc, IntPtr bps, out uint free, IntPtr total);
  public static void Run(string path, string install) {
    var w = new StreamWriter(path);
    foreach (var p in new string[]{
        install + @"\nativePC\rom\Initialize.arc",          // an existing file
        install + @"\nativePC\rom\NoSuchFile.arc",          // a missing file
        install + @"\nativePC\rom",                         // its directory
        install + @"\nativePC\rom\",
        install + @"\DDDA.exe",
        @"E:\" }) {
      uint free = 0x55; SetLastError(0);
      bool ok = GetDiskFreeSpaceA(p, IntPtr.Zero, IntPtr.Zero, out free, IntPtr.Zero);
      w.WriteLine("{0}: {1} err={2} free={3}", p.Replace(install, "<install>"), ok, Marshal.GetLastWin32Error(), ok ? "ok" : free.ToString("X"));
    }
    w.Close();
  }
}
