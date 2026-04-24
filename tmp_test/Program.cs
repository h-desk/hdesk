using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

class WeChatFocusMonitor
{
    [StructLayout(LayoutKind.Sequential)]
    struct GUITHREADINFO
    {
        public int cbSize;
        public uint flags;
        public IntPtr hwndActive;
        public IntPtr hwndFocus;
        public IntPtr hwndCapture;
        public IntPtr hwndMenuOwner;
        public IntPtr hwndMoveSize;
        public IntPtr hwndCaret;
        public int rcCaretL, rcCaretT, rcCaretR, rcCaretB;
    }

    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll", SetLastError = true)] static extern bool GetGUIThreadInfo(uint idThread, ref GUITHREADINFO lpgui);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("user32.dll")] static extern IntPtr GetFocus();
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("imm32.dll")] static extern IntPtr ImmGetContext(IntPtr hWnd);
    [DllImport("imm32.dll")] static extern bool ImmReleaseContext(IntPtr hWnd, IntPtr hIMC);
    [DllImport("imm32.dll")] static extern bool ImmGetOpenStatus(IntPtr hIMC);
    [DllImport("imm32.dll")] static extern IntPtr ImmGetDefaultIMEWnd(IntPtr hWnd);

    static string GetClass(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero) return "null";
        var sb = new StringBuilder(256);
        GetClassName(hwnd, sb, 256);
        return sb.ToString();
    }

    static string GetTitle(IntPtr hwnd)
    {
        if (hwnd == IntPtr.Zero) return "";
        var sb = new StringBuilder(256);
        GetWindowText(hwnd, sb, 256);
        return sb.ToString();
    }

    static string Hex(IntPtr p) { return "0x" + p.ToString("X"); }

    static void Main()
    {
        Console.OutputEncoding = Encoding.UTF8;
        uint myTid = GetCurrentThreadId();
        Console.WriteLine("Monitoring Qt windows. Ctrl+C to exit.");

        while (true)
        {
            IntPtr fg = GetForegroundWindow();
            string fgClass = GetClass(fg);
            string fgUpper = fgClass.ToUpper();

            if (!(fgUpper.StartsWith("QT") || fgUpper.Contains("QWINDOW")))
            {
                Thread.Sleep(200);
                continue;
            }

            uint fgTid = GetWindowThreadProcessId(fg, out uint fgPid);
            string ts = DateTime.Now.ToString("HH:mm:ss.fff");
            string title = GetTitle(fg);

            var gti = new GUITHREADINFO();
            gti.cbSize = Marshal.SizeOf(gti);
            GetGUIThreadInfo(fgTid, ref gti);
            bool caretBlink = (gti.flags & 1) != 0;

            IntPtr hImc = ImmGetContext(fg);
            bool immOpen = false;
            bool hasImm = hImc != IntPtr.Zero;
            if (hasImm) { immOpen = ImmGetOpenStatus(hImc); ImmReleaseContext(fg, hImc); }

            IntPtr imeWnd = ImmGetDefaultIMEWnd(fg);

            IntPtr focusHwnd = IntPtr.Zero;
            string focusClass = "?";
            bool focusImmOpen = false;
            bool focusHasImm = false;

            if (fgTid != myTid)
            {
                if (AttachThreadInput(myTid, fgTid, true))
                {
                    focusHwnd = GetFocus();
                    focusClass = GetClass(focusHwnd);

                    if (focusHwnd != IntPtr.Zero)
                    {
                        IntPtr hImc2 = ImmGetContext(focusHwnd);
                        focusHasImm = hImc2 != IntPtr.Zero;
                        if (focusHasImm) { focusImmOpen = ImmGetOpenStatus(hImc2); ImmReleaseContext(focusHwnd, hImc2); }
                    }

                    AttachThreadInput(myTid, fgTid, false);
                }
            }

            Console.WriteLine("[" + ts + "] " + fgClass + " \"" + title + "\""
                + " | caret=" + Hex(gti.hwndCaret) + " blink=" + caretBlink
                + " rc=(" + gti.rcCaretL + "," + gti.rcCaretT + "," + gti.rcCaretR + "," + gti.rcCaretB + ")"
                + " | gtiFocus=" + Hex(gti.hwndFocus)
                + " | IMM:has=" + hasImm + ",open=" + immOpen
                + " | IMEWnd=" + Hex(imeWnd)
                + " | Focus=" + Hex(focusHwnd) + "[" + focusClass + "]"
                + " imm=" + focusHasImm + "/" + focusImmOpen);

            Thread.Sleep(200);
        }
    }
}
