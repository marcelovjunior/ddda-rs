# Windows behaviour probes (brief L17)

Small C# programs that call the real Windows functions the runtime models
(`crates/ddruntime`: text, system, registry, profile and file services)
and print what Windows answers. They read nothing from the game and write
only into the directory they are given (and the probing process's own
environment). The runtime's doc comments say "measured" where an answer
comes from them; the numbers in `crates/ddruntime/tests/runtime.rs` are
their output on Windows 11 build 26200 (en-US).

Run with Windows PowerShell 5.1 from this directory (`<out>` is any
scratch directory outside the repo):

```powershell
Add-Type -Path Nls.cs; [Nls]::BestFit('<out>\bestfit.txt')        # cp1252 decode + best fit
Add-Type -Path Chars.cs; [Chars]::Run('<out>\chars.txt')          # types, case, sort keys
python gen_text_tables.py <out> ..\..\..\crates\ddruntime\src\text\tables.rs
Add-Type -Path Probe.cs; [Probe]::Run('<out>\probe.txt')          # conversion/string contracts
Add-Type -Path Probe2.cs; [Probe2]::Run('<out>\probe2.txt')       # flags, Global*, env, time, std handles
Add-Type -Path Probe3.cs; [Probe3]::Run('<out>')                  # alloc flags, env order, version, ini files
Add-Type -Path Probe4.cs; [Probe4]::Run('<out>')                  # version hierarchy, ini edge cases
Add-Type -Path Probe5.cs; [Probe5]::Run('<out>')                  # ini lists, nSize 0, deletes
Add-Type -Path Reg.cs; [Reg]::Run('<out>\reg.txt')                # registry contract (read-only)
Add-Type -Path Disk.cs; [Disk]::Run('<out>')                      # GetDiskFreeSpaceA, SHCreateDirectoryExW
Add-Type -Path Vectors.cs; [Vectors]::Run('<out>')                # random differential vectors
```

Two probes must run as console-less GUI programs:

```powershell
Add-Type -Path NoConsole.cs -OutputAssembly <out>\noconsole.exe -OutputType WindowsApplication
Start-Process -Wait <out>\noconsole.exe -ArgumentList <out>\noconsole.txt
Add-Type -Path Unmanifested.cs -OutputAssembly <out>\unmanifested.exe -OutputType WindowsApplication
Start-Process -Wait <out>\unmanifested.exe -ArgumentList <out>\unmanifested.txt
```

`unmanifested.exe` gets the compiler's default manifest, which, like
DDDA.exe's, has no `supportedOS` entries: Windows then reports version 6.2
(build 9200) to it, which is what the game sees.

`Vectors.cs` writes `utf8_decode.txt`, `wc_encode.txt` and `sortkeys.txt`
(about 68,000 cases). Point `DDRUNTIME_WINDOWS_VECTORS` at that directory
and `cargo test -p ddruntime --test runtime conversions_and_sort_keys`
compares the runtime against all of them.
