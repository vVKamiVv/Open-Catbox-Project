// Finds every place in memory that stores the given 8-byte pointer values (little-endian), e.g. vtables pointing at a typeinfo.
// usage (headless): -postScript FindPtr.java <hex value> [<hex value> ...] "<output file>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

import java.io.File;
import java.io.PrintWriter;

public class FindPtr extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            for (int i = 0; i < args.length - 1; i++) {
                long value = Long.parseUnsignedLong(args[i], 16);
                byte[] pattern = new byte[8];
                for (int b = 0; b < 8; b++) {
                    pattern[b] = (byte) (value >>> (8 * b));
                }
                out.println("\n===== " + args[i]);
                Address hit = currentProgram.getMemory().findBytes(currentProgram.getMinAddress(), pattern, null, true, monitor);
                while (hit != null) {
                    Function f = getFunctionContaining(hit);
                    out.println("  stored at " + hit + (f == null ? "" : " (inside " + f.getName() + ")"));
                    hit = currentProgram.getMemory().findBytes(hit.add(1), pattern, null, true, monitor);
                }
            }
        }
    }
}
