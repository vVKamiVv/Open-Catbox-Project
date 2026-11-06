// Decompiles every function of the program into a local folder, for reading and grepping the whole engine at once:
// <out>/index.tsv (entry address, name, file, body size) and <out>/<address block>.c with the functions of each
// 0x10000-byte block in address order. The output is derived from the executable: keep it local, never commit it.
// usage (headless): -postScript DecompileAll.java "<output folder>"
//@category Umineko
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.decompiler.parallel.DecompileConfigurer;
import ghidra.app.decompiler.parallel.DecompilerCallback;
import ghidra.app.decompiler.parallel.ParallelDecompiler;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.util.task.TaskMonitor;

import java.io.File;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

public class DecompileAll extends GhidraScript {
    private record Decompiled(long entry, String name, long size, String text) {}

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        File out = new File(args[0]);
        out.mkdirs();

        List<Function> functions = new ArrayList<>();
        for (Function f : currentProgram.getFunctionManager().getFunctions(true)) {
            if (!f.isExternal() && !f.isThunk()) {
                functions.add(f);
            }
        }
        println("Decompiling " + functions.size() + " functions");

        DecompileConfigurer configurer = decompiler -> {
            DecompileOptions options = new DecompileOptions();
            options.grabFromProgram(currentProgram);
            decompiler.setOptions(options);
            decompiler.toggleCCode(true);
            decompiler.toggleSyntaxTree(false);
            decompiler.setSimplificationStyle("decompile");
        };
        DecompilerCallback<Decompiled> callback = new DecompilerCallback<>(currentProgram, configurer) {
            @Override
            public Decompiled process(DecompileResults results, TaskMonitor monitor) {
                Function f = results.getFunction();
                String text = results.decompileCompleted()
                        ? results.getDecompiledFunction().getC()
                        : "// decompile failed: " + results.getErrorMessage() + "\n";
                return new Decompiled(f.getEntryPoint().getOffset(), f.getName(), f.getBody().getNumAddresses(), text);
            }
        };
        callback.setTimeout(120);
        List<Decompiled> results = ParallelDecompiler.decompileFunctions(callback, functions, monitor);
        callback.dispose();

        Map<Long, List<Decompiled>> blocks = new TreeMap<>();
        for (Decompiled d : results) {
            if (d != null) {
                blocks.computeIfAbsent(d.entry() >>> 16, k -> new ArrayList<>()).add(d);
            }
        }
        try (PrintWriter index = new PrintWriter(new File(out, "index.tsv"), "UTF-8")) {
            index.println("entry\tname\tfile\tsize");
            for (Map.Entry<Long, List<Decompiled>> block : blocks.entrySet()) {
                String file = String.format("%x.c", block.getKey() << 16);
                List<Decompiled> list = block.getValue();
                list.sort((a, b) -> Long.compareUnsigned(a.entry(), b.entry()));
                try (PrintWriter c = new PrintWriter(new File(out, file), "UTF-8")) {
                    for (Decompiled d : list) {
                        c.printf("%n// ===== %s @ %x (%d bytes)%n", d.name(), d.entry(), d.size());
                        c.print(d.text());
                        index.printf("%x\t%s\t%s\t%d%n", d.entry(), d.name(), file, d.size());
                    }
                }
            }
        }
        println("Wrote " + results.size() + " functions into " + blocks.size() + " files");
    }
}
