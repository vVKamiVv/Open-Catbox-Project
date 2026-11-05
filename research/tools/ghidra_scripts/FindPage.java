// Lists every instruction whose operands contain a value inside a 4 KiB page (AArch64 builds addresses with
// adrp + add/ldr, which auto-analysis doesn't always turn into references), with the containing function.
// usage (headless): -postScript FindPage.java <hex page address> [<hex page address> ...] "<output file>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.scalar.Scalar;

import java.io.File;
import java.io.PrintWriter;

public class FindPage extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(new File(args[args.length - 1]), "UTF-8")) {
            for (int i = 0; i < args.length - 1; i++) {
                long page = Long.parseUnsignedLong(args[i], 16) & ~0xfffL;
                out.println("\n===== page " + Long.toHexString(page));
                for (Instruction insn : currentProgram.getListing().getInstructions(true)) {
                    for (int op = 0; op < insn.getNumOperands(); op++) {
                        for (Object o : insn.getOpObjects(op)) {
                            long v = o instanceof Scalar s ? s.getUnsignedValue()
                                    : o instanceof Address a ? a.getOffset() : -1;
                            if ((v & ~0xfffL) == page) {
                                Function f = getFunctionContaining(insn.getAddress());
                                out.println("  " + insn.getAddress() + "  " + insn
                                        + (f == null ? "" : "   in " + f.getName() + "@" + f.getEntryPoint()));
                            }
                        }
                    }
                }
            }
        }
    }
}
