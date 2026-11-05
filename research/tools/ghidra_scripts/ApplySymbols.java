// Applies re/symbols.csv (address,name,comment) to the current program: names functions (creating them if the
// analysis left a bare label) and data, inside C++-style namespaces, and adds the comment as a plate comment.
// usage (headless, without -readOnly so the names are saved): -postScript ApplySymbols.java "<symbols.csv>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.CodeUnit;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Namespace;
import ghidra.program.model.symbol.SourceType;
import ghidra.program.model.symbol.SymbolTable;
import ghidra.app.util.NamespaceUtils;

import java.io.BufferedReader;
import java.io.FileReader;
import java.util.ArrayList;
import java.util.List;

public class ApplySymbols extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        SymbolTable symbols = currentProgram.getSymbolTable();
        int functions = 0, labels = 0, failed = 0;
        try (BufferedReader in = new BufferedReader(new FileReader(args[0], java.nio.charset.StandardCharsets.UTF_8))) {
            in.readLine(); // header
            String line;
            while ((line = in.readLine()) != null) {
                List<String> cols = parseCsv(line);
                if (cols.size() < 2) {
                    continue;
                }
                Address address = toAddr(cols.get(0));
                String fullName = cols.get(1).replace("~", "dtor_");
                String comment = cols.size() > 2 ? cols.get(2) : "";
                int split = fullName.lastIndexOf("::");
                String name = split < 0 ? fullName : fullName.substring(split + 2);
                try {
                    Namespace ns = split < 0 ? currentProgram.getGlobalNamespace()
                            : NamespaceUtils.createNamespaceHierarchy(fullName.substring(0, split), null,
                                    currentProgram, SourceType.USER_DEFINED);
                    MemoryBlock block = currentProgram.getMemory().getBlock(address);
                    if (block != null && block.isExecute()) {
                        Function f = getFunctionAt(address);
                        if (f == null) {
                            disassemble(address);
                            f = createFunction(address, null);
                        }
                        if (f == null) {
                            failed++;
                            continue;
                        }
                        f.setName(name, SourceType.USER_DEFINED);
                        f.setParentNamespace(ns);
                        functions++;
                    } else {
                        symbols.createLabel(address, name, ns, SourceType.USER_DEFINED).setPrimary();
                        labels++;
                    }
                    if (!comment.isEmpty()) {
                        currentProgram.getListing().setComment(address, CodeUnit.PLATE_COMMENT, fullName + ": " + comment);
                    }
                } catch (Exception e) {
                    println("failed " + cols.get(0) + " " + fullName + ": " + e.getMessage());
                    failed++;
                }
            }
        }
        println("applied: " + functions + " functions, " + labels + " labels, " + failed + " failed");
    }

    private static List<String> parseCsv(String line) {
        List<String> out = new ArrayList<>();
        StringBuilder cur = new StringBuilder();
        boolean quoted = false;
        for (int i = 0; i < line.length(); i++) {
            char c = line.charAt(i);
            if (quoted) {
                if (c == '"' && i + 1 < line.length() && line.charAt(i + 1) == '"') {
                    cur.append('"');
                    i++;
                } else if (c == '"') {
                    quoted = false;
                } else {
                    cur.append(c);
                }
            } else if (c == '"') {
                quoted = true;
            } else if (c == ',') {
                out.add(cur.toString());
                cur.setLength(0);
            } else {
                cur.append(c);
            }
        }
        out.add(cur.toString());
        return out;
    }
}
