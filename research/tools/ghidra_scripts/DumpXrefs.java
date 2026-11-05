// Finds defined strings containing any keyword, lists their references, and decompiles the referencing functions.
// usage (headless): -postScript DumpXrefs.java <keyword> [<keyword> ...] "<output file>"
// Keywords are case-insensitive substrings (analyzeHeadless.bat runs through cmd.exe, which mangles regex
// characters and splits arguments on commas), and the last argument is always the output file.
//@category Umineko
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;

import java.io.File;
import java.io.PrintWriter;
import java.util.Arrays;
import java.util.LinkedHashSet;
import java.util.Set;

public class DumpXrefs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        String[] keywords = Arrays.stream(args, 0, args.length - 1).map(String::toLowerCase).toArray(String[]::new);
        Set<Function> functions = new LinkedHashSet<>();
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            for (Data data : currentProgram.getListing().getDefinedData(true)) {
                if (!data.hasStringValue()) {
                    continue;
                }
                Object value = data.getValue();
                String lower = value == null ? "" : value.toString().toLowerCase();
                if (Arrays.stream(keywords).noneMatch(lower::contains)) {
                    continue;
                }
                out.println("STRING " + data.getAddress() + " \"" + value + "\"");
                for (Reference ref : getReferencesTo(data.getAddress())) {
                    Function f = getFunctionContaining(ref.getFromAddress());
                    out.println("  xref " + ref.getFromAddress() + " in "
                            + (f == null ? "?" : f.getName() + "@" + f.getEntryPoint()));
                    if (f != null) {
                        functions.add(f);
                    }
                }
            }
            DecompInterface decompiler = new DecompInterface();
            decompiler.openProgram(currentProgram);
            for (Function f : functions) {
                DecompileResults result = decompiler.decompileFunction(f, 120, monitor);
                out.println("\n===== " + f.getName() + " @ " + f.getEntryPoint());
                out.println(result.decompileCompleted()
                        ? result.getDecompiledFunction().getC()
                        : "decompile failed: " + result.getErrorMessage());
            }
        }
    }
}
