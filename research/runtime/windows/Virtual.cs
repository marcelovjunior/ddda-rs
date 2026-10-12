using System;
using System.IO;
using System.Runtime.InteropServices;
// VirtualAlloc / VirtualFree contracts (L21): reserve vs commit, commits
// inside a reservation, address rounding, decommit, release and their
// errors, as the MtVirtualAllocator (0x00D11B60, 0x00D10E90) uses them.
public static class Virtual {
  [DllImport("kernel32.dll")] static extern void SetLastError(uint e);
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr VirtualAlloc(IntPtr a, UIntPtr n, uint type, uint prot);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool VirtualFree(IntPtr a, UIntPtr n, uint type);
  [StructLayout(LayoutKind.Sequential)] struct MBI { public IntPtr Base, AllocBase; public uint AllocProt; public ushort Part; public UIntPtr Size; public uint State, Prot, Type; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern UIntPtr VirtualQuery(IntPtr a, out MBI m, UIntPtr n);
  static StreamWriter w;
  static long A(string label, long addr, long size, uint type, uint prot) {
    SetLastError(0x77); IntPtr r = VirtualAlloc((IntPtr)addr, (UIntPtr)(ulong)size, type, prot); int e = Marshal.GetLastWin32Error();
    w.WriteLine("ALLOC {0}: r={1} (offset {2}) err={3}", label, r == IntPtr.Zero ? "NULL" : "ok", r == IntPtr.Zero ? "-" : (r.ToInt64() - baseAddr).ToString("X"), e);
    return r.ToInt64();
  }
  static void F(string label, long addr, long size, uint type) {
    SetLastError(0x77); bool r = VirtualFree((IntPtr)addr, (UIntPtr)(ulong)size, type); int e = Marshal.GetLastWin32Error();
    w.WriteLine("FREE {0}: r={1} err={2}", label, r, e);
  }
  static void Q(string label, long addr) {
    MBI m; VirtualQuery((IntPtr)addr, out m, (UIntPtr)(ulong)Marshal.SizeOf(typeof(MBI)));
    w.WriteLine("QUERY {0}: base+{1:X} allocbase+{2:X} size {3:X} state {4:X} prot {5:X}", label, m.Base.ToInt64() - baseAddr, m.AllocBase.ToInt64() - baseAddr, (ulong)m.Size, m.State, m.Prot);
  }
  static long baseAddr;
  public static void Run(string path) {
    w = new StreamWriter(path);
    baseAddr = 0;
    long r = A("reserve 0x20000000", 0, 0x20000000, 0x2000, 4);
    baseAddr = r;
    w.WriteLine("reserve base aligned to 64K: {0}", (r & 0xFFFF) == 0);
    Q("reserved start", r);
    A("commit 0x30000 at +0x10000", r + 0x10000, 0x30000, 0x1000, 4);
    Q("after commit", r + 0x10000);
    A("commit unaligned +0x12345 size 0x10", r + 0x12345, 0x10, 0x1000, 4);
    A("commit straddling page +0x41FF0 size 0x20", r + 0x41FF0, 0x20, 0x1000, 4);
    Q("after straddle", r + 0x41000);
    A("recommit committed +0x10000", r + 0x10000, 0x1000, 0x1000, 4);
    A("commit beyond the reservation", r + 0x20000000, 0x1000, 0x1000, 4);
    A("commit across the end", r + 0x1FFFF000, 0x2000, 0x1000, 4);
    A("reserve inside the reservation", r + 0x100000, 0x10000, 0x2000, 4);
    A("reserve+commit at a used address", r + 0x100000, 0x10000, 0x3000, 4);
    A("size 0", 0, 0, 0x2000, 4);
    A("type 0", 0, 0x1000, 0, 4);
    A("prot 0", 0, 0x1000, 0x2000, 0);
    A("prot 0 commit", r + 0x200000, 0x1000, 0x1000, 0);
    long c = A("commit only, NULL address", 0, 0x5000, 0x1000, 4);
    Q("commit-only region", c);
    w.WriteLine("commit-only aligned to 64K: {0}; same reservation as the big one: {1}", (c & 0xFFFF) == 0, c >= r && c < r + 0x20000000);
    F("decommit +0x10000 size 0x10000", r + 0x10000, 0x10000, 0x4000);
    Q("after decommit", r + 0x10000);
    F("decommit unaligned +0x40010 size 0x10", r + 0x40010, 0x10, 0x4000);
    F("decommit already decommitted", r + 0x10000, 0x1000, 0x4000);
    F("release with size", r, 0x1000, 0x8000);
    F("release not base", r + 0x10000, 0, 0x8000);
    F("decommit|release", r, 0, 0xC000);
    F("type 0", r, 0, 0);
    F("decommit size 0 at base", r, 0, 0x4000);
    Q("after full decommit", r + 0x40000);
    F("release", r, 0, 0x8000);
    Q("after release", r);
    F("release again", r, 0, 0x8000);
    F("release commit-only", c, 0, 0x8000);
    F("release NULL", 0, 0, 0x8000);
    w.Close();
  }
}
