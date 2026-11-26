using System;
using System.Globalization;
using System.Runtime.InteropServices;

class NaveWinMem {
    [StructLayout(LayoutKind.Sequential)]
    struct MEMORYSTATUSEX {
        public uint dwLength;
        public uint dwMemoryLoad;
        public ulong ullTotalPhys;
        public ulong ullAvailPhys;
        public ulong ullTotalPageFile;
        public ulong ullAvailPageFile;
        public ulong ullTotalVirtual;
        public ulong ullAvailVirtual;
        public ulong ullAvailExtendedVirtual;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GlobalMemoryStatusEx(ref MEMORYSTATUSEX lpBuffer);

    static void Main() {
        var m = new MEMORYSTATUSEX { dwLength = (uint)Marshal.SizeOf(typeof(MEMORYSTATUSEX)) };
        if (GlobalMemoryStatusEx(ref m) && m.ullTotalPhys > 0) {
            double tot = m.ullTotalPhys / 1073741824.0;
            double used = (m.ullTotalPhys - m.ullAvailPhys) / 1073741824.0;
            int pct = (int)Math.Round((used / tot) * 100.0);
            Console.WriteLine(string.Format(CultureInfo.InvariantCulture, "{0:F1} {1:F1} {2}", used, tot, pct));
        }
    }
}
