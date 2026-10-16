# Read-only probe of the running 3.3.5a client (no injection): walks the object manager the
# same way the plugin does and prints each step, to verify the offsets in crates/host-wow/src/wow.rs.
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Mem {
  [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(int a, bool b, int pid);
  [DllImport("kernel32.dll")] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, int size, out int read);
}
"@
$p = Get-Process Wow -ErrorAction Stop | Select-Object -First 1
$h = [Mem]::OpenProcess(0x10, $false, $p.Id)
function RdB($a, $n) { $b = New-Object byte[] $n; $r = 0; if (-not [Mem]::ReadProcessMemory($h, [IntPtr]([int64]$a), $b, $n, [ref]$r)) { return $null }; $b }
function U32($a) { $b = RdB $a 4; if ($null -eq $b) { return $null }; [BitConverter]::ToUInt32($b, 0) }
function U64($a) { $b = RdB $a 8; if ($null -eq $b) { return $null }; [BitConverter]::ToUInt64($b, 0) }
function F32($a) { $b = RdB $a 4; if ($null -eq $b) { return $null }; [BitConverter]::ToSingle($b, 0) }
function Hex($v) { if ($null -eq $v) { "unreadable" } else { '0x{0:X}' -f $v } }

$conn = U32 0xC79CE0;               "client connection  : $(Hex $conn)"
$om = U32 ($conn + 0x2ED0);         "object manager     : $(Hex $om)"
$guid = U64 ($om + 0xC0);           "local player guid  : $(Hex $guid)"
$obj = U32 ($om + 0xAC);            "first object       : $(Hex $obj)"
$n = 0; $found = $null
while ($obj -and ($obj -band 1) -eq 0 -and $n -lt 5000) {
  $g = U64 ($obj + 0x30); $t = U32 ($obj + 0x14)
  if ($n -lt 5) { "  obj $(Hex $obj) type $t guid $(Hex $g)" }
  if ($g -eq $guid) { $found = $obj; break }
  $obj = U32 ($obj + 0x3C); $n++
}
"walked $n objects; local player object: $(Hex $found)"
if ($found) {
  "position x/y/z     : $(F32 ($found + 0x798)) $(F32 ($found + 0x79C)) $(F32 ($found + 0x7A0))"
  "facing             : $(F32 ($found + 0x7A8))"
}
"map id (0xAB63BC)  : $(U32 0xAB63BC)"
