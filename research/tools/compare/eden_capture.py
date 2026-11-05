"""Captures the original game in the Eden emulator at given lines of text, for comparison with the port.

How it works: Eden's GDB server (Settings > Debug > "Enable GDB Stub", port 6543) lets this script break in the
game's MSGSET command constructor (`0x7100005140`), which runs once per line of text before it is shown, with the
command arguments in x3 (message id: u32 at x3 + 4, the script's id minus one; ids given and printed here are the
script's, as `sdu` and shin show them). While text skip is on, lines pass at about 50 a second, too fast to stop on
one exactly, so a few lines before each target the script turns skip off (B) and advances line by line (A). On the
target it lets the line type out and settle, takes Eden's own screenshot (Ctrl+P, saved without a dialog when
"save as" is off; at Eden's render scale, 5760x3240 at 3x) as <out>/<id>.png and turns skip back on (R). Keys are
only sent while Eden is the foreground window.

Two quirks of Eden's GDB stub (v0.2.1) shape the breakpoint handling: "continue" does not step over a breakpoint
(the game stops on it again), and after one single-step every later "continue" only steps one instruction. So the
breakpoint stays in place, and on each hit the script performs the constructor's first instruction itself
(`str x19, [sp, #-32]!`) and moves pc past it before continuing. Eden's GDB server accepts no new client once one has
resumed the game and left, so restart Eden before each run.

The port captures the same lines with `shin --capture-messages <ids> --capture-dir captures/port`.

Run it, then start the episode in Eden (Title > Start > Episode > Yes; never Continue) and press R (E on the
keyboard) to skip:
    python -I eden_capture.py --messages 1200,1450 --out captures/original
    python -I eden_capture.py --probe        # check the connection: prints the next 5 message ids, then resumes

Key bindings (keyboard -> Switch): X = A (advance), Z = B (stop skip), E = R (skip on/off). Standard library only.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import os
import re
import shutil
import socket
import sys
import time

# The MSGSET constructor in the v1.0.3 executable, as an offset from the start of the main module, and its first
# instruction (str x19, [sp, #-32]!), which the script performs itself on every hit
MSGSET_CONSTRUCTOR_OFFSET = 0x7100005140 - 0x7100000000
MSGSET_CONSTRUCTOR_FIRST_INSTRUCTION = "f30f1ef8"
MESSAGE_ID_OFFSET = 4  # in the arguments, which x3 points to
SP, PC = 31, 32  # GDB register numbers on aarch64

# Lines before a target at which skip is turned off (skip stops within about 2 lines of holding B); message ids run
# in order within a scene
SLOW_DOWN_LINES = 8

# --- GDB remote serial protocol ---------------------------------------------------------------------------------


class Gdb:
    def __init__(self, host, port):
        deadline = time.time() + 60
        while True:
            try:
                self.sock = socket.create_connection((host, port), timeout=5)
                break
            except OSError:
                if time.time() > deadline:
                    raise SystemExit(f"could not connect to Eden's GDB server at {host}:{port} (restart Eden)")
                time.sleep(1)
        self.buffer = b""

    def _read_byte(self):
        while not self.buffer:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise SystemExit("Eden closed the GDB connection")
            self.buffer += chunk
        byte, self.buffer = self.buffer[:1], self.buffer[1:]
        return byte

    def _read_packet(self):
        while self._read_byte() != b"$":
            pass
        payload = bytearray()
        while (byte := self._read_byte()) != b"#":
            payload += byte
        self._read_byte(), self._read_byte()  # checksum
        self.sock.sendall(b"+")
        return payload.decode("latin-1")

    def send(self, data):
        """Sends a command without waiting for its reply"""
        checksum = sum(data.encode("latin-1")) % 256
        self.sock.sendall(f"${data}#{checksum:02x}".encode("latin-1"))

    def ask(self, data, timeout=10.0):
        self.send(data)
        self.sock.settimeout(timeout)
        try:
            return self._read_packet()
        finally:
            self.sock.settimeout(None)

    def wait_stop(self, timeout=None):
        """The next stop reply, or None if the game is still running after `timeout` seconds"""
        self.sock.settimeout(timeout)
        try:
            return self._read_packet()
        except (socket.timeout, TimeoutError):
            return None
        finally:
            self.sock.settimeout(None)

    def interrupt(self, timeout=3.0):
        """Pauses the game and reads every stop reply that is waiting (a breakpoint hit may have come in just before
        the interrupt's own reply); returns the last one, or None if none came"""
        self.sock.sendall(b"\x03")
        last = self.wait_stop(timeout)
        while last is not None and (more := self.wait_stop(0.5)) is not None:
            last = more
        return last

    def register(self, number):
        return int.from_bytes(bytes.fromhex(self.ask(f"p{number:x}")), "little")

    def set_register(self, number, value):
        if self.ask(f"P{number:x}={value.to_bytes(8, 'little').hex()}") != "OK":
            raise SystemExit(f"Eden refused to write register {number}")

    def memory(self, address, length):
        return bytes.fromhex(self.ask(f"m{address:x},{length:x}"))

    def write_memory(self, address, data):
        if self.ask(f"M{address:x},{len(data):x}:{data.hex()}") != "OK":
            raise SystemExit(f"Eden refused to write memory at {address:#x}")

    def main_module_base(self):
        """Start address of the game's main module: the first one after the loader (Umineko's is named "adv.nss")"""
        xml = self.ask("qXfer:libraries:read::0,ffff")
        modules = re.findall(r'<library name="([^"]*)"><segment address="(0x[0-9a-fA-F]+)"', xml)
        for name, address in modules:
            if "rtld" not in name.lower():
                print(f"main module: {name}")
                return int(address, 16)
        raise SystemExit(f"could not find the main module; library list: {xml!r}")


def pass_breakpoint(gdb, address):
    """Performs `str x19, [sp, #-32]!` (the instruction under the breakpoint) and moves pc to the next one"""
    sp = gdb.register(SP) - 32
    gdb.write_memory(sp, gdb.register(19).to_bytes(8, "little"))
    gdb.set_register(SP, sp)
    gdb.set_register(PC, address + 4)


# --- Eden window and keyboard (Win32) -------------------------------------------------------------------------

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32")
user32.SetProcessDPIAware()
VK = {"x": 0x58, "z": 0x5A, "e": 0x45, "p": 0x50, "ctrl": 0x11}


def find_eden_window():
    """The Eden window running a game (its title ends with "(64-bit)")"""
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def visit(hwnd, _):
        length = user32.GetWindowTextLengthW(hwnd)
        title = ctypes.create_unicode_buffer(length + 1)
        user32.GetWindowTextW(hwnd, title, length + 1)
        if user32.IsWindowVisible(hwnd) and "eden" in title.value.lower() and "(64-bit)" in title.value:
            found.append(hwnd)
        return True

    user32.EnumWindows(visit, 0)
    if not found:
        raise SystemExit("Eden with the game is not running")
    return found[0]


def focus(hwnd):
    foreground = user32.GetForegroundWindow()
    thread = user32.GetWindowThreadProcessId(foreground, None)
    me = kernel32.GetCurrentThreadId()
    user32.AttachThreadInput(me, thread, True)
    user32.SetForegroundWindow(hwnd)
    user32.AttachThreadInput(me, thread, False)
    time.sleep(0.15)
    if user32.GetForegroundWindow() != hwnd:
        # minimise + maximise brings a window to the front when SetForegroundWindow is refused
        user32.ShowWindow(hwnd, 6)
        time.sleep(0.3)
        user32.ShowWindow(hwnd, 3)
        time.sleep(0.5)
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.15)
    if user32.GetForegroundWindow() != hwnd:
        raise SystemExit("could not bring Eden to the front; refusing to send input")


def key(hwnd, name, down):
    # never type into another application
    if user32.GetForegroundWindow() != hwnd:
        raise SystemExit("Eden is not the foreground window; refusing to send input")
    code = VK[name]
    user32.keybd_event(code, user32.MapVirtualKeyW(code, 0), 0 if down else 2, 0)


def press(hwnd, *names):
    for name in names:
        key(hwnd, name, True)
    time.sleep(0.07)
    for name in reversed(names):
        key(hwnd, name, False)
    time.sleep(0.1)


PNG_END = bytes.fromhex("0000000049454e44ae426082")  # an empty IEND chunk


def take_screenshot(hwnd, folder, timeout=20.0):
    """Presses Ctrl+P and returns the new file Eden saves"""
    before = set(os.listdir(folder)) if os.path.isdir(folder) else set()
    press(hwnd, "ctrl", "p")
    deadline = time.time() + timeout
    while time.time() < deadline:
        new = [f for f in (os.listdir(folder) if os.path.isdir(folder) else []) if f not in before]
        if new:
            path = os.path.join(folder, new[0])
            # Eden takes a few seconds to write the PNG at 3x scale: wait for its final (IEND) chunk
            while time.time() < deadline:
                if os.path.getsize(path) > len(PNG_END):
                    with open(path, "rb") as file:
                        file.seek(-len(PNG_END), os.SEEK_END)
                        if file.read() == PNG_END:
                            return path
                time.sleep(0.2)
            break
        time.sleep(0.1)
    raise SystemExit(f"Eden did not save a screenshot in {folder}")


# --- main -----------------------------------------------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--messages", default="", help="comma-separated message ids to capture, in script order")
    parser.add_argument("--out", default="captures/original")
    parser.add_argument("--settle", type=float, default=4.0, help="seconds to let a line type out and settle")
    parser.add_argument("--screenshots", default=os.path.expandvars(r"%APPDATA%\eden\screenshots"))
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=6543)
    parser.add_argument("--probe", action="store_true", help="print the next 5 message ids, then resume")
    args = parser.parse_args()

    gdb = Gdb(args.host, args.port)
    print("stop reason:", gdb.ask("?"))
    gdb.interrupt()
    breakpoint_address = gdb.main_module_base() + MSGSET_CONSTRUCTOR_OFFSET
    found = gdb.memory(breakpoint_address, 4).hex()
    if found != MSGSET_CONSTRUCTOR_FIRST_INSTRUCTION:
        raise SystemExit(f"unexpected code at the MSGSET constructor ({found}): not version 1.0.3, or a breakpoint "
                         "from an earlier run is still there (restart Eden)")
    print(f"MSGSET constructor at {breakpoint_address:#x}")
    if gdb.ask(f"Z0,{breakpoint_address:x},4") != "OK":
        raise SystemExit("Eden refused the breakpoint")

    remaining = [int(m) for m in args.messages.split(",") if m]
    os.makedirs(args.out, exist_ok=True)
    try:
        capture_loop(gdb, args, breakpoint_address, remaining)
    finally:
        gdb.interrupt(timeout=1.0)
        print("remove breakpoint:", gdb.ask(f"z0,{breakpoint_address:x},4"))
        if gdb.register(PC) == breakpoint_address:
            pass_breakpoint(gdb, breakpoint_address)
        gdb.send("c")
        gdb.sock.close()
    print("done; restart Eden before the next run")


class Lines:
    """The game's lines of text as they start, seen through the breakpoint"""

    def __init__(self, gdb, breakpoint_address):
        self.gdb, self.address = gdb, breakpoint_address

    def _line_at(self, stop):
        """The message id of the line whose breakpoint the game stopped on (passed, game still paused)"""
        pc = self.gdb.register(PC)
        if pc != self.address:
            print(f"unexpected stop at {pc:#x}: {stop}")
            return None
        message_id = int.from_bytes(self.gdb.memory(self.gdb.register(3) + MESSAGE_ID_OFFSET, 4), "little")
        pass_breakpoint(self.gdb, self.address)
        # the game passes the script's message id minus one (script 109 "Nanjo: ...You've..." arrives as 108)
        return message_id + 1

    def next(self, timeout=None):
        """Waits for the next line (game running) and returns its id with the game paused, or None on timeout"""
        while (stop := self.gdb.wait_stop(timeout)) is not None:
            if (message_id := self._line_at(stop)) is not None:
                return message_id
            self.gdb.send("c")
        return None

    def run(self, seconds):
        """Lets the running game go on for `seconds`, passing every line on the way; returns their ids. The game keeps
        running afterwards. Breakpoint stops are short, so the game keeps drawing frames and reading input."""
        started, deadline = [], time.time() + seconds
        while (left := deadline - time.time()) > 0 and (message_id := self.next(left)) is not None:
            started.append(message_id)
            self.gdb.send("c")
        return started


STARTED = time.time()


def log(text):
    print(f"{time.time() - STARTED:8.2f} s  {text}")


def capture_loop(gdb, args, breakpoint_address, remaining):
    lines = Lines(gdb, breakpoint_address)
    gdb.send("c")
    if args.probe:
        for _ in range(5):
            message_id = lines.next()
            text = gdb.memory(int.from_bytes(gdb.memory(gdb.register(3) + 0x10, 8), "little"), 96)
            print(f"line {message_id}: {text.split(b'\0')[0].decode('latin-1')!r}")
            gdb.send("c")
        return

    hwnd = find_eden_window()
    skipping, current, paused = True, 0, False  # the user turns skip on after starting the episode
    while remaining:
        target = remaining[0]
        if skipping:
            # skip until a few lines before the target
            while (current := lines.next()) < target - SLOW_DOWN_LINES:
                gdb.send("c")
            # stop skipping: hold B while the game runs past the next breakpoints (each stops it only briefly)
            focus(hwnd)
            key(hwnd, "z", True)
            gdb.send("c")
            passed = lines.run(0.5)
            key(hwnd, "z", False)
            current, paused, skipping = max([current] + passed), False, False
            log(f"line {current}: stopped skipping for {target}; lines passed meanwhile: {passed}")
        # advance line by line; the first A press may only finish typing out the current line
        while current < target:
            focus(hwnd)
            press(hwnd, "x")
            if (message_id := lines.next(0.6)) is not None:
                current, paused = message_id, True
                log(f"line {current}")
                if current < target:
                    gdb.send("c")
                    paused = False
        if paused:
            gdb.send("c")
        if current > target:
            log(f"line {current} came instead of {target}; not captured")
        else:
            # let the line type out, with the game running so Eden keeps drawing; lines that start meanwhile
            # advanced on their own
            if extra := lines.run(args.settle):
                log(f"lines {extra} started on their own after {target}; the capture shows the last of them")
            path = os.path.join(args.out, f"{target}.png")
            screenshot = take_screenshot(hwnd, args.screenshots)
            shutil.copyfile(screenshot, path)
            try:
                os.remove(screenshot)
            except PermissionError:
                pass  # Eden may still hold it open; it is a copy anyway
            log(f"captured line {target}: {path}")
        remaining.pop(0)
        if remaining and remaining[0] - current > SLOW_DOWN_LINES:
            focus(hwnd)
            press(hwnd, "e")  # skip on again
            skipping = True


if __name__ == "__main__":
    sys.exit(main())
