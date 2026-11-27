using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

class NaveHideMouse {
    const uint SPI_SETCURSORS = 0x0057;
    const uint OCR_NORMAL = 32512;
    const uint OCR_IBEAM = 32513;
    const uint OCR_HAND = 32649;
    const int SM_CXCURSOR = 13;
    const int SM_CYCURSOR = 14;
    const int VK_LBUTTON = 0x01;
    const int VK_RBUTTON = 0x02;
    const int VK_MBUTTON = 0x04;

    [StructLayout(LayoutKind.Sequential)]
    struct POINT {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    delegate bool ConsoleCtrlDelegate(int ctrlType);

    [DllImport("user32.dll")]
    static extern bool SetProcessDPIAware();

    [DllImport("user32.dll")]
    static extern IntPtr SetProcessDpiAwarenessContext(IntPtr dpiContext);

    [DllImport("user32.dll")]
    static extern uint GetDpiForWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    static extern int GetSystemMetrics(int nIndex);

    [DllImport("user32.dll")]
    static extern bool GetCursorPos(out POINT lpPoint);

    [DllImport("user32.dll")]
    static extern short GetAsyncKeyState(int vKey);

    [DllImport("user32.dll")]
    static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);

    [DllImport("user32.dll")]
    static extern bool GetClientRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    static extern bool ClientToScreen(IntPtr hWnd, ref POINT lpPoint);

    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr CreateCursor(
        IntPtr hInst, int xHotSpot, int yHotSpot, int nWidth, int nHeight,
        byte[] pvANDPlane, byte[] pvXORPlane);

    [DllImport("user32.dll", SetLastError = true)]
    static extern IntPtr CopyIcon(IntPtr hIcon);

    [DllImport("user32.dll", SetLastError = true)]
    static extern bool DestroyCursor(IntPtr hCursor);

    [DllImport("user32.dll", SetLastError = true)]
    static extern bool SetSystemCursor(IntPtr hcur, uint id);

    [DllImport("user32.dll", SetLastError = true)]
    static extern bool SystemParametersInfo(uint uiAction, uint uiParam, IntPtr pvParam, uint fWinIni);

    [DllImport("kernel32.dll")]
    static extern bool SetConsoleCtrlHandler(ConsoleCtrlDelegate handler, bool add);

    static readonly object StateLock = new object();
    static volatile bool _hidden = false;
    static int _stop = 0;
    static int _hideNow = 0;
    static IntPtr _blankCursor = IntPtr.Zero;
    static ConsoleCtrlDelegate _ctrlHandler;

    static IntPtr _cachedHwnd = IntPtr.Zero;
    static bool _cachedIsTerm = false;

    static void EnableDpiAwareness() {
        try {
            // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = (IntPtr)(-4)
            SetProcessDpiAwarenessContext(new IntPtr(-4));
        } catch {
            try { SetProcessDPIAware(); } catch { }
        }
    }

    static bool IsTerminalWindow(IntPtr hwnd) {
        if (hwnd == IntPtr.Zero) return false;
        if (hwnd == _cachedHwnd) return _cachedIsTerm;

        bool isTerm = false;
        try {
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (pid != 0) {
                using (var proc = Process.GetProcessById((int)pid)) {
                    string name = proc.ProcessName;
                    isTerm =
                        name.Equals("WindowsTerminal", StringComparison.OrdinalIgnoreCase) ||
                        name.Equals("OpenConsole", StringComparison.OrdinalIgnoreCase) ||
                        name.Equals("conhost", StringComparison.OrdinalIgnoreCase) ||
                        name.Equals("wezterm-gui", StringComparison.OrdinalIgnoreCase) ||
                        name.Equals("alacritty", StringComparison.OrdinalIgnoreCase);
                }
            }
        } catch {
            isTerm = false;
        }
        _cachedHwnd = hwnd;
        _cachedIsTerm = isTerm;
        return isTerm;
    }

    static bool IsInsideEditorArea(IntPtr hwnd, POINT pt) {
        RECT rc;
        if (!GetClientRect(hwnd, out rc)) return false;
        POINT tl = new POINT { X = rc.Left, Y = rc.Top };
        POINT br = new POINT { X = rc.Right, Y = rc.Bottom };
        if (!ClientToScreen(hwnd, ref tl) || !ClientToScreen(hwnd, ref br)) return false;

        // Keep Windows Terminal tab bar & title buttons visible when hovering top chrome
        uint dpi = 96;
        try {
            uint d = GetDpiForWindow(hwnd);
            if (d >= 72 && d <= 480) dpi = d;
        } catch { }
        int topChrome = (int)((44 * dpi) / 96);

        return pt.X >= tl.X && pt.X < br.X && pt.Y >= (tl.Y + topChrome) && pt.Y < br.Y;
    }

    static bool AnyMouseButtonDown() {
        return (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0
            || (GetAsyncKeyState(VK_RBUTTON) & 0x8000) != 0
            || (GetAsyncKeyState(VK_MBUTTON) & 0x8000) != 0;
    }

    static IntPtr BuildBlankCursor() {
        int w = GetSystemMetrics(SM_CXCURSOR);
        int h = GetSystemMetrics(SM_CYCURSOR);
        if (w <= 0 || w > 128) w = 32;
        if (h <= 0 || h > 128) h = 32;
        int stride = ((w + 15) / 16) * 2;
        byte[] andMask = new byte[stride * h];
        for (int i = 0; i < andMask.Length; i++) andMask[i] = 0xFF;
        byte[] xorMask = new byte[stride * h];
        return CreateCursor(IntPtr.Zero, 0, 0, w, h, andMask, xorMask);
    }

    static void HideCursors() {
        lock (StateLock) {
            if (_hidden || _blankCursor == IntPtr.Zero) return;
            uint[] ids = { OCR_IBEAM, OCR_NORMAL, OCR_HAND };
            foreach (uint id in ids) {
                IntPtr copy = CopyIcon(_blankCursor);
                if (copy != IntPtr.Zero) {
                    if (!SetSystemCursor(copy, id)) {
                        DestroyCursor(copy);
                    }
                }
            }
            _hidden = true;
        }
    }

    static void RestoreCursors(bool force) {
        lock (StateLock) {
            if (!force && !_hidden) return;
            SystemParametersInfo(SPI_SETCURSORS, 0, IntPtr.Zero, 0);
            _hidden = false;
        }
    }

    static int Main(string[] args) {
        EnableDpiAwareness();

        if (args.Length > 0 && args[0].Equals("--reset", StringComparison.OrdinalIgnoreCase)) {
            RestoreCursors(true);
            return 0;
        }

        if (args.Length > 0 && args[0].Equals("--self-test", StringComparison.OrdinalIgnoreCase)) {
            IntPtr c = BuildBlankCursor();
            if (c == IntPtr.Zero) {
                Console.Error.WriteLine("CreateCursor failed");
                return 1;
            }
            IntPtr copy = CopyIcon(c);
            bool ok = copy != IntPtr.Zero;
            if (copy != IntPtr.Zero) DestroyCursor(copy);
            DestroyCursor(c);
            POINT pt;
            bool hasPos = GetCursorPos(out pt);
            Console.WriteLine("OK cursor=" + ok + " pos=" + hasPos + " (" + pt.X + "," + pt.Y + ")");
            return (ok && hasPos) ? 0 : 1;
        }

        int idleMs = 500;
        if (args.Length > 0) {
            int parsed;
            if (int.TryParse(args[0], out parsed) && parsed >= 100 && parsed <= 10000) {
                idleMs = parsed;
            }
        }

        _ctrlHandler = ctrl => {
            Interlocked.Exchange(ref _stop, 1);
            RestoreCursors(false);
            return false;
        };
        SetConsoleCtrlHandler(_ctrlHandler, true);
        AppDomain.CurrentDomain.ProcessExit += (s, e) => RestoreCursors(false);

        // Watch stdin pipe from nave-edit: 'H' = hide now on keypress, 'Q'/EOF = exit & restore immediately
        Thread stdinWatcher = new Thread(() => {
            try {
                Stream s = Console.OpenStandardInput();
                byte[] buf = new byte[64];
                while (Volatile.Read(ref _stop) == 0) {
                    int n = s.Read(buf, 0, buf.Length);
                    if (n <= 0) break;
                    for (int i = 0; i < n; i++) {
                        if (buf[i] == (byte)'H') {
                            Interlocked.Exchange(ref _hideNow, 1);
                        } else if (buf[i] == (byte)'Q') {
                            Interlocked.Exchange(ref _stop, 1);
                            return;
                        }
                    }
                }
            } catch {
            } finally {
                Interlocked.Exchange(ref _stop, 1);
            }
        });
        stdinWatcher.IsBackground = true;
        stdinWatcher.Start();

        // Serialize across multiple ne tabs so only one helper manages system cursors at a time
        bool ownsMutex = false;
        using (Mutex mtx = new Mutex(false, "Local\\NaveEditHideMouseMutex")) {
            try {
                while (Volatile.Read(ref _stop) == 0 && !ownsMutex) {
                    try {
                        ownsMutex = mtx.WaitOne(80);
                    } catch (AbandonedMutexException) {
                        ownsMutex = true;
                        RestoreCursors(true);
                    }
                }
                if (!ownsMutex) return 0;

                _blankCursor = BuildBlankCursor();
                if (_blankCursor == IntPtr.Zero) return 1;

                Stopwatch sw = Stopwatch.StartNew();
                POINT lastPt;
                GetCursorPos(out lastPt);
                long lastMoveMs = -idleMs; // Hide right away on launch if pointer is already idle inside editor

                while (Volatile.Read(ref _stop) == 0) {
                    POINT pt;
                    if (!GetCursorPos(out pt)) {
                        Thread.Sleep(25);
                        continue;
                    }

                    long now = sw.ElapsedMilliseconds;
                    int dist = Math.Abs(pt.X - lastPt.X) + Math.Abs(pt.Y - lastPt.Y);
                    if (dist >= 2) {
                        lastPt = pt;
                        lastMoveMs = now;
                        Interlocked.Exchange(ref _hideNow, 0);
                        RestoreCursors(false);
                        Thread.Sleep(25);
                        continue;
                    }

                    IntPtr fg = GetForegroundWindow();
                    bool eligible = IsTerminalWindow(fg) && IsInsideEditorArea(fg, pt) && !AnyMouseButtonDown();
                    if (!eligible) {
                        Interlocked.Exchange(ref _hideNow, 0);
                        RestoreCursors(false);
                        Thread.Sleep(35);
                        continue;
                    }

                    bool immediate = Interlocked.Exchange(ref _hideNow, 0) != 0;
                    if (immediate || (now - lastMoveMs) >= idleMs) {
                        HideCursors();
                    }

                    Thread.Sleep(25);
                }
            } finally {
                RestoreCursors(false);
                if (_blankCursor != IntPtr.Zero) {
                    DestroyCursor(_blankCursor);
                    _blankCursor = IntPtr.Zero;
                }
                if (ownsMutex) {
                    try { mtx.ReleaseMutex(); } catch { }
                }
            }
        }
        return 0;
    }
}
