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
    const int IDC_SIZEWE = 32644;
    const int IDC_SIZENS = 32645;
    const int IDC_HAND = 32649;
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
    static extern IntPtr LoadCursor(IntPtr hInstance, IntPtr lpCursorName);

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
    static int _appliedShape = 'D';
    static int _desiredShape = 'D';
    static int _autoHide = 1;
    static int _stop = 0;
    static int _hideNow = 0;
    static IntPtr _blankCursor = IntPtr.Zero;
    static IntPtr _curWE = IntPtr.Zero;
    static IntPtr _curNS = IntPtr.Zero;
    static IntPtr _curHand = IntPtr.Zero;
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

    static IntPtr CacheStandardCursor(int idc) {
        IntPtr shared = LoadCursor(IntPtr.Zero, new IntPtr(idc));
        if (shared == IntPtr.Zero) return IntPtr.Zero;
        return CopyIcon(shared);
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
            _appliedShape = 0;
        }
    }

    static void ApplyShape(int shape) {
        lock (StateLock) {
            if (shape == 'D') {
                if (_hidden || _appliedShape != 'D') {
                    SystemParametersInfo(SPI_SETCURSORS, 0, IntPtr.Zero, 0);
                    _hidden = false;
                    _appliedShape = 'D';
                }
                return;
            }
            IntPtr src = IntPtr.Zero;
            if (shape == 'E') src = _curWE;
            else if (shape == 'N') src = _curNS;
            else if (shape == 'P') src = _curHand;
            if (src == IntPtr.Zero) {
                SystemParametersInfo(SPI_SETCURSORS, 0, IntPtr.Zero, 0);
                _hidden = false;
                _appliedShape = 'D';
                return;
            }
            if (!_hidden && _appliedShape == shape) return;

            uint[] targets = { OCR_IBEAM, OCR_NORMAL };
            foreach (uint id in targets) {
                IntPtr copy = CopyIcon(src);
                if (copy != IntPtr.Zero) {
                    if (!SetSystemCursor(copy, id)) {
                        DestroyCursor(copy);
                    }
                }
            }
            // If we were hidden, also restore OCR_HAND so system hand cursor isn't blank
            if (_hidden && _curHand != IntPtr.Zero) {
                IntPtr copyH = CopyIcon(_curHand);
                if (copyH != IntPtr.Zero) {
                    if (!SetSystemCursor(copyH, OCR_HAND)) {
                        DestroyCursor(copyH);
                    }
                }
            }
            _hidden = false;
            _appliedShape = shape;
        }
    }

    static void RestoreCursors(bool force) {
        lock (StateLock) {
            if (!force && !_hidden && _appliedShape == 'D') return;
            SystemParametersInfo(SPI_SETCURSORS, 0, IntPtr.Zero, 0);
            _hidden = false;
            _appliedShape = 'D';
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
            IntPtr we = CacheStandardCursor(IDC_SIZEWE);
            IntPtr ns = CacheStandardCursor(IDC_SIZENS);
            IntPtr hd = CacheStandardCursor(IDC_HAND);
            bool ok = c != IntPtr.Zero && we != IntPtr.Zero && ns != IntPtr.Zero && hd != IntPtr.Zero;
            if (c != IntPtr.Zero) DestroyCursor(c);
            if (we != IntPtr.Zero) DestroyCursor(we);
            if (ns != IntPtr.Zero) DestroyCursor(ns);
            if (hd != IntPtr.Zero) DestroyCursor(hd);
            POINT pt;
            bool hasPos = GetCursorPos(out pt);
            Console.WriteLine("OK cursors=" + ok + " pos=" + hasPos + " (" + pt.X + "," + pt.Y + ")");
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
            RestoreCursors(true);
            return false;
        };
        SetConsoleCtrlHandler(_ctrlHandler, true);
        AppDomain.CurrentDomain.ProcessExit += (s, e) => RestoreCursors(true);

        // Pipe protocol from nave-edit:
        // 'H'=hide now on key, 'A'/'a'=auto-hide ON/OFF, 'D'=default, 'E'=↔, 'N'=↕, 'P'=hand, 'Q'/EOF=exit
        Thread stdinWatcher = new Thread(() => {
            try {
                Stream s = Console.OpenStandardInput();
                byte[] buf = new byte[64];
                while (Volatile.Read(ref _stop) == 0) {
                    int n = s.Read(buf, 0, buf.Length);
                    if (n <= 0) break;
                    for (int i = 0; i < n; i++) {
                        byte b = buf[i];
                        if (b == (byte)'H') {
                            Interlocked.Exchange(ref _hideNow, 1);
                        } else if (b == (byte)'A') {
                            Interlocked.Exchange(ref _autoHide, 1);
                        } else if (b == (byte)'a') {
                            Interlocked.Exchange(ref _autoHide, 0);
                            Interlocked.Exchange(ref _hideNow, 0);
                        } else if (b == (byte)'D' || b == (byte)'E' || b == (byte)'N' || b == (byte)'P') {
                            Interlocked.Exchange(ref _desiredShape, (int)b);
                        } else if (b == (byte)'Q') {
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

                // Ensure clean baseline before caching standard OS cursors
                RestoreCursors(true);
                _blankCursor = BuildBlankCursor();
                _curWE = CacheStandardCursor(IDC_SIZEWE);
                _curNS = CacheStandardCursor(IDC_SIZENS);
                _curHand = CacheStandardCursor(IDC_HAND);
                if (_blankCursor == IntPtr.Zero) return 1;

                Stopwatch sw = Stopwatch.StartNew();
                POINT lastPt;
                GetCursorPos(out lastPt);
                long lastMoveMs = -idleMs;

                while (Volatile.Read(ref _stop) == 0) {
                    POINT pt;
                    if (!GetCursorPos(out pt)) {
                        Thread.Sleep(20);
                        continue;
                    }

                    IntPtr fg = GetForegroundWindow();
                    bool inside = IsTerminalWindow(fg) && IsInsideEditorArea(fg, pt);
                    if (!inside) {
                        Interlocked.Exchange(ref _hideNow, 0);
                        RestoreCursors(false);
                        Thread.Sleep(30);
                        continue;
                    }

                    long now = sw.ElapsedMilliseconds;
                    int dist = Math.Abs(pt.X - lastPt.X) + Math.Abs(pt.Y - lastPt.Y);
                    bool moved = dist >= 2;
                    if (moved) {
                        lastPt = pt;
                        lastMoveMs = now;
                        Interlocked.Exchange(ref _hideNow, 0);
                    }

                    int desired = Volatile.Read(ref _desiredShape);
                    bool autoHide = Volatile.Read(ref _autoHide) != 0;
                    bool btnDown = AnyMouseButtonDown();

                    if (moved || btnDown || !autoHide) {
                        ApplyShape(desired);
                        Thread.Sleep(20);
                        continue;
                    }

                    if (!_hidden && _appliedShape != desired) {
                        ApplyShape(desired);
                    }

                    bool immediate = Interlocked.Exchange(ref _hideNow, 0) != 0;
                    if (immediate || (now - lastMoveMs) >= idleMs) {
                        HideCursors();
                    }

                    Thread.Sleep(20);
                }
            } finally {
                RestoreCursors(true);
                if (_blankCursor != IntPtr.Zero) { DestroyCursor(_blankCursor); _blankCursor = IntPtr.Zero; }
                if (_curWE != IntPtr.Zero) { DestroyCursor(_curWE); _curWE = IntPtr.Zero; }
                if (_curNS != IntPtr.Zero) { DestroyCursor(_curNS); _curNS = IntPtr.Zero; }
                if (_curHand != IntPtr.Zero) { DestroyCursor(_curHand); _curHand = IntPtr.Zero; }
                if (ownsMutex) {
                    try { mtx.ReleaseMutex(); } catch { }
                }
            }
        }
        return 0;
    }
}
