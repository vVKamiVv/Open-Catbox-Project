// Finds every occurrence of hex byte patterns in the loaded program memory (e.g. magic numbers of embedded blobs).
// usage (headless): -postScript FindBytes.java <hex bytes> [<hex bytes> ...] "<output file>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;

import java.io.File;
import java.io.PrintWriter;

public class FindBytes extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            for (int i = 0; i < args.length - 1; i++) {
                String hex = args[i];
                byte[] pattern = new byte[hex.length() / 2];
                for (int b = 0; b < pattern.length; b++) {
                    pattern[b] = (byte) Integer.parseInt(hex.substring(b * 2, b * 2 + 2), 16);
                }
                out.println("\n===== " + hex);
                int count = 0;
                Address hit = currentProgram.getMemory().findBytes(currentProgram.getMinAddress(), pattern, null, true, monitor);
                while (hit != null && count < 200) {
                    out.println("  at " + hit + " block " + currentProgram.getMemory().getBlock(hit).getName());
                    count++;
                    hit = currentProgram.getMemory().findBytes(hit.add(1), pattern, null, true, monitor);
                }
            }
        }
    }
}
