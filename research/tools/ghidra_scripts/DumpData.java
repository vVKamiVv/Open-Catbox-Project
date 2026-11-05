// Prints 16 bytes at each address as hex, as four little-endian floats/ints, and as a C string.
// usage (headless): -postScript DumpData.java <hex address> [<hex address> ...] "<output file>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;

import java.io.File;
import java.io.PrintWriter;

public class DumpData extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            for (int i = 0; i < args.length - 1; i++) {
                Address address = toAddr(args[i]);
                byte[] bytes = getBytes(address, 16);
                StringBuilder hex = new StringBuilder();
                StringBuilder floats = new StringBuilder();
                StringBuilder ints = new StringBuilder();
                for (int b = 0; b < 16; b++) {
                    hex.append(String.format("%02x ", bytes[b]));
                }
                for (int w = 0; w < 4; w++) {
                    int bits = (bytes[w * 4] & 0xff) | (bytes[w * 4 + 1] & 0xff) << 8
                            | (bytes[w * 4 + 2] & 0xff) << 16 | (bytes[w * 4 + 3] & 0xff) << 24;
                    floats.append(Float.intBitsToFloat(bits)).append("  ");
                    ints.append(bits).append("  ");
                }
                StringBuilder str = new StringBuilder();
                for (int b = 0; b < 16 && bytes[b] >= 0x20; b++) {
                    str.append((char) bytes[b]);
                }
                out.printf("%s  hex: %s%n    f32: %s%n    i32: %s%n    str: \"%s\"%n", address, hex, floats, ints, str);
            }
        }
    }
}
