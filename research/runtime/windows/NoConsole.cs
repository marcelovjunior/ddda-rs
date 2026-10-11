using System;
using System.IO;
using System.Runtime.InteropServices;
public static class NoConsole {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GetStdHandle(int n);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetConsoleMode(IntPtr h, out uint mode);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint GetConsoleCP();
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool WriteConsoleW(IntPtr h, char[] buf, uint n, IntPtr written, IntPtr reserved);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr GetConsoleWindow();
  public static void Main(string[] args) {
    var w = new StreamWriter(args[0]);
    w.WriteLine("console window {0:X}", (long)GetConsoleWindow());
    for (int n = -10; n >= -12; n--) { SetLastError(0); IntPtr h = GetStdHandle(n); w.WriteLine("GetStdHandle({0}) = {1:X} err={2}", n, (long)h, Marshal.GetLastWin32Error()); }
    foreach (long hv in new long[]{0, -1, 0x1234}) {
      uint mode = 0x55; SetLastError(0); bool r = GetConsoleMode((IntPtr)hv, out mode); w.WriteLine("GetConsoleMode({0:X}) = {1} err={2} mode={3:X}", hv, r, Marshal.GetLastWin32Error(), mode);
      IntPtr wr = Marshal.AllocHGlobal(4); Marshal.WriteInt32(wr, 0x55);
      SetLastError(0); bool r2 = WriteConsoleW((IntPtr)hv, "hi".ToCharArray(), 2, wr, IntPtr.Zero); w.WriteLine("WriteConsoleW({0:X}) = {1} err={2} written={3:X}", hv, r2, Marshal.GetLastWin32Error(), Marshal.ReadInt32(wr));
    }
    SetLastError(0); uint cp = GetConsoleCP(); w.WriteLine("GetConsoleCP = {0} err={1}", cp, Marshal.GetLastWin32Error());
    w.Close();
  }
}
