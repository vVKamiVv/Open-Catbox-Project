// Decompiles the functions containing the given addresses.
// usage (headless): -postScript DecompileAt.java "<output file>" <hex address> [<hex address> ...]
//@category Umineko
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

import java.io.File;
import java.io.PrintWriter;

public class DecompileAt extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);
        try (PrintWriter out = new PrintWriter(new File(args[0]), "UTF-8")) {
            for (int i = 1; i < args.length; i++) {
                Address address = toAddr(args[i]);
                Function f = getFunctionContaining(address);
                if (f == null) {
                    // vtable targets the auto-analysis left as bare labels
                    disassemble(address);
                    f = createFunction(address, null);
                }
                if (f == null) {
                    out.println("\n===== no function at " + address);
                    continue;
                }
                DecompileResults result = decompiler.decompileFunction(f, 120, monitor);
                out.println("\n===== " + f.getName() + " @ " + f.getEntryPoint() + " (asked " + address + ")");
                out.println(result.decompileCompleted()
                        ? result.getDecompiledFunction().getC()
                        : "decompile failed: " + result.getErrorMessage());
            }
        }
    }
}
