// Extracts the embedded NVN shaders of the given program ids. The engine's static program table
// (shin::gProgramTable, 0x71001de1c0, 48-byte entries) points at a vertex blob (+0) and a fragment blob (+8) per
// program; each blob's header gives a base offset (+0x94), the control section (+0xc0 offset, +0xcc size) and the
// GPU code section (+0xc4 offset, +0xc8 size), as consumed by the program setup at 0x71000cea50.
// Writes <outdir>/program_<id>_<vs|fs>.code and .ctrl and prints the header fields.
// usage (headless): -postScript ExtractShaders.java <hex table address> <program id>... "<output dir>"
//@category Umineko
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;

import java.io.File;
import java.io.FileOutputStream;
import java.io.PrintWriter;

public class ExtractShaders extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        Address table = toAddr(args[0]);
        File outDir = new File(args[args.length - 1]);
        outDir.mkdirs();
        try (PrintWriter log = new PrintWriter(new File(outDir, "extract.log"), "UTF-8")) {
            for (int i = 1; i < args.length - 1; i++) {
                int id = Integer.decode(args[i]);
                Address entry = table.add(id * 48L);
                String[] stages = {"vs", "fs"};
                for (int s = 0; s < 2; s++) {
                    Address blob = toAddr(getLong(entry.add(s * 8L)));
                    long base = getInt(blob.add(0x94)) & 0xffffffffL;
                    long ctrlOff = getInt(blob.add(0xc0)) & 0xffffffffL;
                    long codeOff = getInt(blob.add(0xc4)) & 0xffffffffL;
                    long codeSize = getInt(blob.add(0xc8)) & 0xffffffffL;
                    long ctrlSize = getInt(blob.add(0xcc)) & 0xffffffffL;
                    log.printf("program 0x%x %s: blob %s base 0x%x code @0x%x size 0x%x, control @0x%x size 0x%x%n",
                            id, stages[s], blob, base, codeOff, codeSize, ctrlOff, ctrlSize);
                    write(new File(outDir, String.format("program_%02x_%s.code", id, stages[s])),
                            getBytes(blob.add(base + codeOff), (int) codeSize));
                    write(new File(outDir, String.format("program_%02x_%s.ctrl", id, stages[s])),
                            getBytes(blob.add(base + ctrlOff), (int) ctrlSize));
                }
            }
        }
    }

    private static void write(File file, byte[] data) throws Exception {
        try (FileOutputStream out = new FileOutputStream(file)) {
            out.write(data);
        }
    }
}
