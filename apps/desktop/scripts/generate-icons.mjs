import fs from "node:fs";
import path from "node:path";
import { PNG } from "pngjs";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(here, "..", "src-tauri", "icons");
fs.mkdirSync(outDir, { recursive: true });

const sourceBuffer = fs.readFileSync(path.join(outDir, "icon-source.png"));
const source = PNG.sync.read(sourceBuffer);
fs.writeFileSync(path.join(outDir, "icon.png"), sourceBuffer);

function resizeBox(src, size) {
    const dst = new PNG({ width: size, height: size });
    const scale = src.width / size;
    for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
            const x0 = Math.floor(x * scale);
            const y0 = Math.floor(y * scale);
            const x1 = Math.min(src.width, Math.ceil((x + 1) * scale));
            const y1 = Math.min(src.height, Math.ceil((y + 1) * scale));
            let r = 0, g = 0, b = 0, a = 0, n = 0;
            for (let sy = y0; sy < y1; sy++) {
                for (let sx = x0; sx < x1; sx++) {
                    const i = (sy * src.width + sx) * 4;
                    r += src.data[i];
                    g += src.data[i + 1];
                    b += src.data[i + 2];
                    a += src.data[i + 3];
                    n++;
                }
            }
            const o = (y * size + x) * 4;
            dst.data[o] = r / n;
            dst.data[o + 1] = g / n;
            dst.data[o + 2] = b / n;
            dst.data[o + 3] = a / n;
        }
    }
    return dst;
}

function bmpEntry(png) {
    const width = png.width;
    const height = png.height;
    const xorStride = width * 4;
    const andStride = Math.ceil(width / 32) * 4;
    const xor = Buffer.alloc(xorStride * height);
    const and = Buffer.alloc(andStride * height);
    for (let y = 0; y < height; y++) {
        const srcRow = height - 1 - y;
        for (let x = 0; x < width; x++) {
            const d = y * xorStride + x * 4;
            const s = (srcRow * width + x) * 4;
            xor[d] = png.data[s + 2];
            xor[d + 1] = png.data[s + 1];
            xor[d + 2] = png.data[s];
            xor[d + 3] = png.data[s + 3];
        }
    }
    const header = Buffer.alloc(40);
    header.writeUInt32LE(40, 0);
    header.writeInt32LE(width, 4);
    header.writeInt32LE(height * 2, 8);
    header.writeUInt16LE(1, 12);
    header.writeUInt16LE(32, 14);
    header.writeUInt32LE(0, 16);
    header.writeUInt32LE(xor.length + and.length, 20);
    return Buffer.concat([header, xor, and]);
}

const SIZES = [16, 24, 32, 48, 64, 128, 256];
const images = SIZES.map((size) => bmpEntry(resizeBox(source, size)));

const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(SIZES.length, 4);
const directory = Buffer.alloc(SIZES.length * 16);
SIZES.forEach((size, i) => {
    const offset = 6 + SIZES.length * 16;
    const entry = i * 16;
    directory[entry] = size >= 256 ? 0 : size;
    directory[entry + 1] = size >= 256 ? 0 : size;
    directory[entry + 2] = 0;
    directory[entry + 3] = 0;
    directory.writeUInt16LE(1, entry + 4);
    directory.writeUInt16LE(32, entry + 6);
    directory.writeUInt32LE(images[i].length, entry + 8);
    directory.writeUInt32LE(offset + images.slice(0, i).reduce((a, b) => a + b.length, 0), entry + 12);
});
fs.writeFileSync(path.join(outDir, "icon.ico"), Buffer.concat([header, directory, ...images]));
console.log("icons generated from official source:", outDir);
